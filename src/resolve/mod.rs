//! Stage 2 — resolve the HCL expressions of `ressources` / `flows` into a
//! plain intermediate representation that `build` can consume without knowing
//! anything about HCL.
//!
//! Resolution is **best-effort**: a reference that cannot be resolved degrades
//! into a [`Endpoint::Symbolic`] node, it never aborts the program. When a
//! value *is* resolved from a named reference, the reference is kept as the
//! endpoint's origin (rendered as the node's description).
//!
//! The cascade is: literal → `local.*` → `var.*` → `module.*.output` (via the
//! `.terraform/modules` cache) → symbolic.

use crate::parse::{self, ModuleArgs, ModuleScope, ParsedConfig};
use hcl::{Expression, FuncCall, Object, ObjectKey, Traversal, TraversalOperator};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// An internal security group from `var.ressources`.
pub struct ResolvedResource {
    pub key: String,
    pub name: String,
    pub type_: String,
    /// The VPC the resource lives in, from the module's `vpc` argument: its
    /// resolved value, or the origin reference when the value is unknowable in
    /// static analysis. `None` only when the argument is absent.
    pub vpc: Option<String>,
}

/// One end of a flow.
pub enum Endpoint {
    /// A reference to an internal resource by its key.
    Resource(String),
    /// A set of CIDR blocks. `origin` is the reference it was resolved from.
    Cidrs {
        values: Vec<String>,
        origin: Option<String>,
    },
    /// A set of prefix-list ids.
    PrefixList {
        values: Vec<String>,
        origin: Option<String>,
    },
    /// An unresolved reference, kept symbolically (e.g. a `data.*` source).
    Symbolic { reference: String },
}

pub use crate::model::PortSpec;

/// A flow, oriented source → destination.
pub struct ResolvedFlow {
    pub name: String,
    pub from: Endpoint,
    pub to: Endpoint,
    pub port: PortSpec,
    pub protocol: String,
}

/// Directives read from a `locals { flowghetti = { ... } }` block in the root
/// module. This is valid, inert Terraform (an unused local that never reaches
/// the real module input); flowghetti interprets it to shape the graph.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FlowghettiConfig {
    /// `merge`: aliased resource key → the canonical key it folds into. The two
    /// (and any further) keys collapse to a single node.
    pub merge: BTreeMap<String, String>,
    /// `groups`: zone name → the **origin references** (as written in the HCL,
    /// e.g. `"local.office_cidr"`, `"module.cidrs.office"`) of the external
    /// endpoints to route into that named zone instead of `External`.
    pub groups: BTreeMap<String, Vec<String>>,
}

/// Resolve every glowwiththeflow module instance in the parsed config, plus the
/// root's `local.flowghetti` directives.
pub fn resolve(
    config: &ParsedConfig,
) -> (Vec<ResolvedResource>, Vec<ResolvedFlow>, FlowghettiConfig) {
    let resolver = Resolver {
        root: &config.scope,
        modules: load_module_cache(&config.root_dir),
        module_args: &config.module_args,
    };

    let mut resources = Vec::new();
    let mut flows = Vec::new();
    for module in &config.modules {
        let vpc = module
            .vpc
            .as_ref()
            .and_then(|expr| resolver.resolve_vpc(expr));
        resolve_resources(&module.ressources, vpc.as_deref(), &mut resources);
        resolver.resolve_flows(&module.flows, &mut flows);
    }
    let directives = read_flowghetti_config(&config.scope);
    (resources, flows, directives)
}

/// Read the `local.flowghetti` directives from the root scope. Best-effort: an
/// absent, non-object, or otherwise malformed block yields an empty config —
/// never an error, never a panic (invariant: best-effort resolution).
fn read_flowghetti_config(scope: &ModuleScope) -> FlowghettiConfig {
    let mut config = FlowghettiConfig::default();
    let Some(root) = scope.locals.get("flowghetti").and_then(as_object) else {
        return config;
    };
    if let Some(merge) = get(root, "merge").and_then(as_object) {
        for (key, value) in merge.iter() {
            if let (Some(key), Some(value)) = (object_key_str(key), as_string(value)) {
                config.merge.insert(key, value);
            }
        }
    }
    if let Some(groups) = get(root, "groups").and_then(as_object) {
        for (name, value) in groups.iter() {
            if let (Some(name), Some(origins)) = (object_key_str(name), as_string_array(value)) {
                config.groups.insert(name, origins);
            }
        }
    }
    config
}

/// A resolution context: the module being resolved, plus — when inside a
/// submodule reached via `module.*.output` — the arguments its caller passed
/// and the caller's scope (so `var.*` resolves to the caller's value).
struct Ctx<'a> {
    module: &'a ModuleScope,
    args: Option<&'a BTreeMap<String, Expression>>,
    caller: Option<&'a ModuleScope>,
}

/// Resolves expressions against the root scope, hopping into cached module
/// scopes to follow `module.*.output` references.
struct Resolver<'a> {
    root: &'a ModuleScope,
    /// Module call name → its parsed scope, from `.terraform/modules`.
    modules: BTreeMap<String, ModuleScope>,
    /// Module call name → the arguments passed by its caller.
    module_args: &'a ModuleArgs,
}

impl Resolver<'_> {
    fn resolve_flows(&self, expr: &Expression, out: &mut Vec<ResolvedFlow>) {
        let Some(obj) = as_object(expr) else {
            return;
        };
        for (name, value) in obj.iter() {
            let Some(name) = object_key_str(name) else {
                continue;
            };
            let Some(fields) = as_object(value) else {
                continue;
            };
            let (Some(from), Some(to)) = (self.resolve_source(fields), self.resolve_dest(fields))
            else {
                continue;
            };
            out.push(ResolvedFlow {
                name,
                from,
                to,
                port: resolve_port(fields),
                protocol: get(fields, "protocol")
                    .and_then(as_string)
                    .unwrap_or_else(|| "tcp".to_string()),
            });
        }
    }

    /// Resolve the module `vpc` to a zone identity, best-effort. A concrete
    /// value when it resolves (literal or `local`/`var`); otherwise the **origin
    /// reference** itself (e.g. `module.generic_aws_vpc.vpc_id`) — so resources
    /// are still grouped even when the id is unknowable in static analysis (a
    /// module output backed by a resource attribute, a `data.*`, …). `None` only
    /// when the expression is neither resolvable nor a reference.
    fn resolve_vpc(&self, expr: &Expression) -> Option<String> {
        self.resolve_scalar(expr).or_else(|| reference_str(expr))
    }

    /// Resolve a single scalar string: a literal, or one level of `local.*` /
    /// `var.*` indirection in the root scope. Anything else yields `None`.
    fn resolve_scalar(&self, expr: &Expression) -> Option<String> {
        match expr {
            Expression::String(s) => Some(s.clone()),
            Expression::Traversal(traversal) => match local_or_var_ref(traversal)? {
                (RefKind::Local, name) => self.resolve_scalar(self.root.locals.get(name)?),
                (RefKind::Var, name) => self.resolve_scalar(self.root.variables.get(name)?),
            },
            _ => None,
        }
    }

    fn resolve_source(&self, fields: &Object<ObjectKey, Expression>) -> Option<Endpoint> {
        if let Some(from) = get(fields, "from").and_then(as_string) {
            return Some(Endpoint::Resource(from));
        }
        if let Some(expr) = get(fields, "from_cidr") {
            return Some(self.list_endpoint(expr, false));
        }
        if let Some(expr) = get(fields, "from_prefix_list") {
            return Some(self.list_endpoint(expr, true));
        }
        None
    }

    fn resolve_dest(&self, fields: &Object<ObjectKey, Expression>) -> Option<Endpoint> {
        if let Some(to) = get(fields, "to").and_then(as_string) {
            return Some(Endpoint::Resource(to));
        }
        if let Some(expr) = get(fields, "to_cidr") {
            return Some(self.list_endpoint(expr, false));
        }
        None
    }

    fn list_endpoint(&self, expr: &Expression, prefix: bool) -> Endpoint {
        let ctx = Ctx {
            module: self.root,
            args: None,
            caller: None,
        };
        let mut visited = Vec::new();
        let (values, origin) = self.resolve_string_list(expr, &ctx, &mut visited);
        match values {
            Some(values) if prefix => Endpoint::PrefixList { values, origin },
            Some(values) => Endpoint::Cidrs { values, origin },
            None => Endpoint::Symbolic {
                reference: origin.unwrap_or_else(|| "<unresolved>".to_string()),
            },
        }
    }

    /// Resolve an expression to a list of strings within `scope`, returning the
    /// resolved values (if any) and the origin reference (if it came from a
    /// named reference).
    fn resolve_string_list(
        &self,
        expr: &Expression,
        ctx: &Ctx,
        visited: &mut Vec<String>,
    ) -> (Option<Vec<String>>, Option<String>) {
        match expr {
            Expression::Array(items) => {
                let mut values = Vec::new();
                for item in items {
                    if let Some(s) = as_string(item) {
                        values.push(s);
                    } else if let (Some(list), _) = self.resolve_string_list(item, ctx, visited) {
                        // a reference/expression that resolves to a list (e.g. `local.*`)
                        values.extend(list);
                    } else if let Some(reference) = reference_str(item) {
                        // an opaque reference kept as a value (e.g. a `data.*.id`)
                        values.push(reference);
                    } else {
                        return (None, None);
                    }
                }
                (Some(values), None)
            }
            Expression::FuncCall(call) => (self.eval_func(call, ctx, visited), None),
            Expression::Traversal(traversal) => {
                let reference = traversal_str(traversal);
                if let Some((module, output)) = module_output_ref(traversal) {
                    return (
                        self.resolve_module_output(&module, &output, ctx, visited),
                        Some(reference),
                    );
                }
                let resolved = match local_or_var_ref(traversal) {
                    Some((RefKind::Local, name)) => ctx
                        .module
                        .locals
                        .get(name)
                        .and_then(|target| self.resolve_string_list(target, ctx, visited).0),
                    Some((RefKind::Var, name)) => self.resolve_var(name, ctx, visited),
                    None => None,
                };
                (resolved, Some(reference))
            }
            Expression::Variable(var) => (None, Some(var.as_str().to_string())),
            _ => (None, None),
        }
    }

    /// Resolve `var.<name>`: a caller-supplied argument (resolved in the
    /// caller's scope) takes precedence over the module's own default.
    fn resolve_var(&self, name: &str, ctx: &Ctx, visited: &mut Vec<String>) -> Option<Vec<String>> {
        if let Some(arg) = ctx.args.and_then(|args| args.get(name)) {
            let caller = Ctx {
                module: ctx.caller.unwrap_or(ctx.module),
                args: None,
                caller: None,
            };
            self.resolve_string_list(arg, &caller, visited).0
        } else {
            let default = ctx.module.variables.get(name)?;
            self.resolve_string_list(default, ctx, visited).0
        }
    }

    /// Resolve `module.<module>.<output>` by following the output's value
    /// expression in the cached module's scope, with the caller's arguments
    /// bound for `var.*`. Cycle-guarded.
    fn resolve_module_output(
        &self,
        module: &str,
        output: &str,
        ctx: &Ctx,
        visited: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        let key = format!("{module}.{output}");
        if visited.contains(&key) {
            return None;
        }
        let sub = self.modules.get(module)?;
        let value = sub.outputs.get(output)?;
        let sub_ctx = Ctx {
            module: sub,
            args: self.module_args.get(module),
            caller: Some(ctx.module),
        };
        visited.push(key);
        let result = self.resolve_string_list(value, &sub_ctx, visited).0;
        visited.pop();
        result
    }

    /// Evaluate the subset of HCL functions used to compose CIDR lists
    /// (`flatten`, `concat`, `distinct`, `compact`, `sort`, `toset`, `tolist`,
    /// `setunion`). Any other function is left unresolved.
    fn eval_func(
        &self,
        call: &FuncCall,
        ctx: &Ctx,
        visited: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        let name = call.name.name.as_str();
        let mut values = match name {
            "flatten" | "concat" | "compact" | "tolist" | "toset" | "setunion" | "distinct"
            | "sort" => self.combine_lists(&call.args, ctx, visited)?,
            _ => return None,
        };
        if matches!(name, "distinct" | "toset" | "setunion") {
            let mut seen = HashSet::new();
            values.retain(|v| seen.insert(v.clone()));
        }
        if name == "compact" {
            values.retain(|v| !v.is_empty());
        }
        if matches!(name, "sort" | "toset" | "setunion") {
            values.sort();
        }
        Some(values)
    }

    /// Resolve and concatenate every argument into a single list of strings.
    fn combine_lists(
        &self,
        args: &[Expression],
        ctx: &Ctx,
        visited: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        let mut values = Vec::new();
        for arg in args {
            values.extend(self.resolve_string_list(arg, ctx, visited).0?);
        }
        Some(values)
    }
}

/// Load the `.terraform/modules` cache into a name → scope map. Best-effort:
/// a missing or malformed manifest yields an empty cache, and module dirs that
/// fail to parse are skipped.
fn load_module_cache(root_dir: &Path) -> BTreeMap<String, ModuleScope> {
    let manifest_path = root_dir.join(".terraform/modules/modules.json");
    let Ok(text) = std::fs::read_to_string(&manifest_path) else {
        return BTreeMap::new();
    };
    let Ok(manifest) = serde_json::from_str::<ModulesManifest>(&text) else {
        return BTreeMap::new();
    };

    let mut cache = BTreeMap::new();
    for entry in manifest.modules {
        if entry.key.is_empty() {
            continue;
        }
        if let Ok(scope) = parse::load_scope(&root_dir.join(&entry.dir)) {
            cache.insert(entry.key, scope);
        }
    }
    cache
}

#[derive(serde::Deserialize)]
struct ModulesManifest {
    #[serde(rename = "Modules")]
    modules: Vec<ModuleEntry>,
}

#[derive(serde::Deserialize)]
struct ModuleEntry {
    #[serde(rename = "Key")]
    key: String,
    #[serde(rename = "Dir")]
    dir: String,
}

fn resolve_resources(expr: &Expression, vpc: Option<&str>, out: &mut Vec<ResolvedResource>) {
    let Some(obj) = as_object(expr) else {
        return;
    };
    for (key, value) in obj.iter() {
        let Some(key) = object_key_str(key) else {
            continue;
        };
        let Some(fields) = as_object(value) else {
            continue;
        };
        let name = get(fields, "name")
            .and_then(as_string)
            .unwrap_or_else(|| key.clone());
        let type_ = get(fields, "type").and_then(as_string).unwrap_or_default();
        out.push(ResolvedResource {
            key,
            name,
            type_,
            vpc: vpc.map(str::to_string),
        });
    }
}

fn resolve_port(fields: &Object<ObjectKey, Expression>) -> PortSpec {
    if let Some(port) = get(fields, "port").and_then(as_number) {
        return PortSpec::Single(port);
    }
    if let Some(range) = get(fields, "port_range").and_then(as_object) {
        let from = get(range, "from").and_then(as_number).unwrap_or(0);
        let to = get(range, "to").and_then(as_number).unwrap_or(0);
        return PortSpec::Range(from, to);
    }
    PortSpec::Any
}

/// Match a `module.<name>.<output>` traversal.
fn module_output_ref(traversal: &Traversal) -> Option<(String, String)> {
    let Expression::Variable(root) = &traversal.expr else {
        return None;
    };
    if root.as_str() != "module" {
        return None;
    }
    match traversal.operators.as_slice() {
        [
            TraversalOperator::GetAttr(name),
            TraversalOperator::GetAttr(output),
        ] => Some((name.as_str().to_string(), output.as_str().to_string())),
        _ => None,
    }
}

enum RefKind {
    Local,
    Var,
}

/// Match a single-level `local.<name>` / `var.<name>` traversal.
fn local_or_var_ref(traversal: &Traversal) -> Option<(RefKind, &str)> {
    let Expression::Variable(root) = &traversal.expr else {
        return None;
    };
    let [TraversalOperator::GetAttr(name)] = traversal.operators.as_slice() else {
        return None;
    };
    match root.as_str() {
        "local" => Some((RefKind::Local, name.as_str())),
        "var" => Some((RefKind::Var, name.as_str())),
        _ => None,
    }
}

/// Render a reference expression (traversal or variable) as a string.
fn reference_str(expr: &Expression) -> Option<String> {
    match expr {
        Expression::Traversal(traversal) => Some(traversal_str(traversal)),
        Expression::Variable(var) => Some(var.as_str().to_string()),
        _ => None,
    }
}

fn traversal_str(traversal: &Traversal) -> String {
    let mut out = reference_root(&traversal.expr);
    for op in &traversal.operators {
        match op {
            TraversalOperator::GetAttr(id) => {
                out.push('.');
                out.push_str(id.as_str());
            }
            TraversalOperator::Index(Expression::String(key)) => {
                out.push_str(&format!("[\"{key}\"]"));
            }
            TraversalOperator::Index(Expression::Number(n)) => {
                if let Some(i) = n.as_i64() {
                    out.push_str(&format!("[{i}]"));
                }
            }
            TraversalOperator::LegacyIndex(i) => out.push_str(&format!("[{i}]")),
            _ => {}
        }
    }
    out
}

fn reference_root(expr: &Expression) -> String {
    match expr {
        Expression::Variable(var) => var.as_str().to_string(),
        other => reference_str(other).unwrap_or_default(),
    }
}

// --- small HCL helpers ----------------------------------------------------

fn as_object(expr: &Expression) -> Option<&Object<ObjectKey, Expression>> {
    match expr {
        Expression::Object(obj) => Some(obj),
        _ => None,
    }
}

fn as_string(expr: &Expression) -> Option<String> {
    match expr {
        Expression::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// An array of string literals as a `Vec<String>`. `None` if `expr` is not an
/// array, or if any element is not a plain string.
fn as_string_array(expr: &Expression) -> Option<Vec<String>> {
    match expr {
        Expression::Array(items) => items.iter().map(as_string).collect(),
        _ => None,
    }
}

fn as_number(expr: &Expression) -> Option<i64> {
    match expr {
        Expression::Number(n) => n.as_i64(),
        _ => None,
    }
}

fn object_key_str(key: &ObjectKey) -> Option<String> {
    match key {
        ObjectKey::Identifier(id) => Some(id.as_str().to_string()),
        ObjectKey::Expression(Expression::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn get<'a>(obj: &'a Object<ObjectKey, Expression>, name: &str) -> Option<&'a Expression> {
    obj.iter()
        .find(|(key, _)| object_key_str(key).as_deref() == Some(name))
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a single HCL expression (the right-hand side of `x = ...`).
    fn expr(src: &str) -> Expression {
        let body = hcl::parse(&format!("x = {src}\n")).expect("valid HCL");
        body.iter()
            .filter_map(|s| s.as_attribute())
            .next()
            .expect("one attribute")
            .expr
            .clone()
    }

    fn local_scope(pairs: &[(&str, &str)]) -> ModuleScope {
        let mut scope = ModuleScope::default();
        for (name, value) in pairs {
            scope.locals.insert((*name).to_string(), expr(value));
        }
        scope
    }

    /// Resolve `src` against `root` with no caller arguments.
    fn resolve_in(
        resolver: &Resolver,
        root: &ModuleScope,
        src: &str,
    ) -> (Option<Vec<String>>, Option<String>) {
        let ctx = Ctx {
            module: root,
            args: None,
            caller: None,
        };
        let mut visited = Vec::new();
        resolver.resolve_string_list(&expr(src), &ctx, &mut visited)
    }

    #[test]
    fn resolve_scalar_handles_literal_local_var_and_unresolvable() {
        let mut scope = local_scope(&[("vpc_id", r#""vpc-fromlocal""#)]);
        scope
            .variables
            .insert("vpc".to_string(), expr(r#""vpc-fromvar""#));
        let args = ModuleArgs::new();
        let resolver = Resolver {
            root: &scope,
            modules: BTreeMap::new(),
            module_args: &args,
        };

        assert_eq!(
            resolver
                .resolve_scalar(&expr(r#""vpc-literal""#))
                .as_deref(),
            Some("vpc-literal")
        );
        assert_eq!(
            resolver.resolve_scalar(&expr("local.vpc_id")).as_deref(),
            Some("vpc-fromlocal")
        );
        assert_eq!(
            resolver.resolve_scalar(&expr("var.vpc")).as_deref(),
            Some("vpc-fromvar")
        );
        // unknown reference degrades to None, never an error.
        assert_eq!(resolver.resolve_scalar(&expr("data.aws_vpc.x.id")), None);
        assert_eq!(resolver.resolve_scalar(&expr("local.missing")), None);
    }

    #[test]
    fn resolve_vpc_falls_back_to_origin_reference() {
        let scope = ModuleScope::default();
        let args = ModuleArgs::new();
        let resolver = Resolver {
            root: &scope,
            modules: BTreeMap::new(),
            module_args: &args,
        };

        // a concrete value wins
        assert_eq!(
            resolver.resolve_vpc(&expr(r#""vpc-123""#)).as_deref(),
            Some("vpc-123")
        );
        // an unresolvable module output keeps the resources grouped under the
        // reference rather than dropping the zone entirely.
        assert_eq!(
            resolver
                .resolve_vpc(&expr("module.generic_aws_vpc.vpc_id"))
                .as_deref(),
            Some("module.generic_aws_vpc.vpc_id")
        );
        assert_eq!(
            resolver
                .resolve_vpc(&expr("data.aws_vpc.this.id"))
                .as_deref(),
            Some("data.aws_vpc.this.id")
        );
    }

    #[test]
    fn resolves_local_list_keeping_origin() {
        let scope = local_scope(&[("vpn", r#"["10.0.0.0/8", "172.16.0.0/12"]"#)]);
        let args = ModuleArgs::new();
        let resolver = Resolver {
            root: &scope,
            modules: BTreeMap::new(),
            module_args: &args,
        };
        let (values, origin) = resolve_in(&resolver, &scope, "local.vpn");
        assert_eq!(
            values,
            Some(vec!["10.0.0.0/8".to_string(), "172.16.0.0/12".to_string()])
        );
        assert_eq!(origin.as_deref(), Some("local.vpn"));
    }

    #[test]
    fn evaluates_distinct_flatten() {
        let scope = local_scope(&[
            ("a", r#"["198.51.100.0/24"]"#),
            ("b", r#"["198.51.100.0/24", "203.0.113.0/24"]"#),
        ]);
        let args = ModuleArgs::new();
        let resolver = Resolver {
            root: &scope,
            modules: BTreeMap::new(),
            module_args: &args,
        };
        let (values, _) = resolve_in(&resolver, &scope, "distinct(flatten([local.a, local.b]))");
        assert_eq!(
            values,
            Some(vec![
                "198.51.100.0/24".to_string(),
                "203.0.113.0/24".to_string()
            ])
        );
    }

    #[test]
    fn resolves_module_output_in_submodule_scope() {
        let mut sub = ModuleScope::default();
        sub.locals
            .insert("net".to_string(), expr(r#"["203.0.113.0/24"]"#));
        sub.outputs.insert("office".to_string(), expr("local.net"));

        let mut modules = BTreeMap::new();
        modules.insert("cidrs".to_string(), sub);
        let root = ModuleScope::default();
        let args = ModuleArgs::new();
        let resolver = Resolver {
            root: &root,
            modules,
            module_args: &args,
        };

        let (values, origin) = resolve_in(&resolver, &root, "module.cidrs.office");
        assert_eq!(values, Some(vec!["203.0.113.0/24".to_string()]));
        assert_eq!(origin.as_deref(), Some("module.cidrs.office"));
    }

    #[test]
    fn resolves_module_output_using_caller_argument() {
        // Submodule: output "net" { value = var.cidrs } with no default.
        let mut sub = ModuleScope::default();
        sub.outputs.insert("net".to_string(), expr("var.cidrs"));
        let mut modules = BTreeMap::new();
        modules.insert("m".to_string(), sub);

        // The caller passes cidrs = local.base, resolved in the caller's scope.
        let mut root = ModuleScope::default();
        root.locals
            .insert("base".to_string(), expr(r#"["203.0.113.0/24"]"#));

        let mut m_args = BTreeMap::new();
        m_args.insert("cidrs".to_string(), expr("local.base"));
        let mut args = ModuleArgs::new();
        args.insert("m".to_string(), m_args);

        let resolver = Resolver {
            root: &root,
            modules,
            module_args: &args,
        };

        let (values, origin) = resolve_in(&resolver, &root, "module.m.net");
        assert_eq!(values, Some(vec!["203.0.113.0/24".to_string()]));
        assert_eq!(origin.as_deref(), Some("module.m.net"));
    }

    #[test]
    fn unresolved_reference_degrades_to_symbolic() {
        let root = ModuleScope::default();
        let args = ModuleArgs::new();
        let resolver = Resolver {
            root: &root,
            modules: BTreeMap::new(),
            module_args: &args,
        };
        let (values, origin) = resolve_in(&resolver, &root, "data.aws_x.y.id");
        assert_eq!(values, None);
        assert_eq!(origin.as_deref(), Some("data.aws_x.y.id"));
    }

    #[test]
    fn array_keeps_opaque_reference_as_value() {
        let root = ModuleScope::default();
        let args = ModuleArgs::new();
        let resolver = Resolver {
            root: &root,
            modules: BTreeMap::new(),
            module_args: &args,
        };
        let (values, _) = resolve_in(&resolver, &root, "[data.aws_prefix_list.x.id]");
        assert_eq!(values, Some(vec!["data.aws_prefix_list.x.id".to_string()]));
    }

    #[test]
    fn reads_flowghetti_merge_directives() {
        let mut scope = ModuleScope::default();
        scope.locals.insert(
            "flowghetti".to_string(),
            expr(r#"{ merge = { aliasone = "canon", aliastwo = "canon" } }"#),
        );

        let config = read_flowghetti_config(&scope);

        assert_eq!(
            config.merge.get("aliasone").map(String::as_str),
            Some("canon")
        );
        assert_eq!(
            config.merge.get("aliastwo").map(String::as_str),
            Some("canon")
        );
        assert_eq!(config.merge.len(), 2);
    }

    #[test]
    fn reads_flowghetti_groups_directives() {
        let mut scope = ModuleScope::default();
        scope.locals.insert(
            "flowghetti".to_string(),
            expr(
                r#"{ groups = { "Office" = ["local.office_cidr"], "VPN" = ["local.vpn_cidr", "module.cidrs.vpn"] } }"#,
            ),
        );

        let config = read_flowghetti_config(&scope);

        assert_eq!(
            config.groups.get("Office").map(Vec::as_slice),
            Some(["local.office_cidr".to_string()].as_slice())
        );
        assert_eq!(
            config.groups.get("VPN").map(Vec::as_slice),
            Some(["local.vpn_cidr".to_string(), "module.cidrs.vpn".to_string()].as_slice())
        );
    }

    #[test]
    fn missing_or_malformed_flowghetti_config_is_empty() {
        // absent entirely
        assert_eq!(
            read_flowghetti_config(&ModuleScope::default()),
            FlowghettiConfig::default()
        );

        // present but not an object → ignored, no panic
        let mut bad = ModuleScope::default();
        bad.locals
            .insert("flowghetti".to_string(), expr(r#""not an object""#));
        assert!(read_flowghetti_config(&bad).merge.is_empty());

        // object without a `merge` key → empty merge
        let mut no_merge = ModuleScope::default();
        no_merge
            .locals
            .insert("flowghetti".to_string(), expr(r#"{ other = "x" }"#));
        assert!(read_flowghetti_config(&no_merge).merge.is_empty());
    }

    #[test]
    fn traversal_str_renders_full_path() {
        let Expression::Traversal(traversal) = expr("module.cidrs.office") else {
            panic!("expected a traversal");
        };
        assert_eq!(traversal_str(&traversal), "module.cidrs.office");
    }
}
