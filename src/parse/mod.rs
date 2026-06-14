//! Stage 1 — load and parse the HCL of a Terraform root directory.
//!
//! This stage knows nothing of the glowwiththeflow domain beyond locating the
//! module call: it loads every `*.tf` file into a [`ModuleScope`] (the `locals`,
//! `variable` defaults and `output` values used for reference resolution) and
//! extracts the `ressources` / `flows` arguments of each glowwiththeflow module
//! block as raw, unevaluated HCL expressions.

use crate::error::{Error, Result};
use hcl::{Body, Expression};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The resolvable bindings of a single Terraform module directory.
#[derive(Default)]
pub struct ModuleScope {
    /// `local.<name>` definitions.
    pub locals: BTreeMap<String, Expression>,
    /// `var.<name>` default values.
    pub variables: BTreeMap<String, Expression>,
    /// `output.<name>` value expressions.
    pub outputs: BTreeMap<String, Expression>,
}

/// Arguments passed to module calls: module name → (argument name → expression).
pub type ModuleArgs = BTreeMap<String, BTreeMap<String, Expression>>;

/// The parsed root module, with everything `resolve` needs.
pub struct ParsedConfig {
    pub root_dir: PathBuf,
    pub scope: ModuleScope,
    /// Every glowwiththeflow module instance found in the root.
    pub modules: Vec<FlowModule>,
    /// Arguments passed to every module call (for `module.*.output` resolution).
    pub module_args: ModuleArgs,
}

/// A single `module "..." { ressources = ...; flows = ... }` call.
pub struct FlowModule {
    pub name: String,
    pub ressources: Expression,
    pub flows: Expression,
    /// The `vpc` argument, if present — the VPC the module's resources live in.
    /// Kept as a raw expression; `resolve` evaluates it best-effort.
    pub vpc: Option<Expression>,
}

/// Load and parse the root directory, extracting its scope and the
/// glowwiththeflow module instances.
pub fn load(dir: &Path) -> Result<ParsedConfig> {
    let mut scope = ModuleScope::default();
    let mut modules = Vec::new();
    let mut module_args = ModuleArgs::new();

    for body in parse_dir(dir)? {
        collect_scope(&body, &mut scope);
        collect_modules(&body, &mut modules, &mut module_args);
    }

    // `*.tfvars` override the `variable` defaults collected above.
    for (name, value) in load_tfvars(dir)? {
        scope.variables.insert(name, value);
    }

    if modules.is_empty() {
        return Err(Error::NoModule);
    }
    Ok(ParsedConfig {
        root_dir: dir.to_path_buf(),
        scope,
        modules,
        module_args,
    })
}

/// Load and parse an arbitrary module directory into its [`ModuleScope`],
/// ignoring any module calls. Used to resolve `module.*.output` references.
pub fn load_scope(dir: &Path) -> Result<ModuleScope> {
    let mut scope = ModuleScope::default();
    for body in parse_dir(dir)? {
        collect_scope(&body, &mut scope);
    }
    Ok(scope)
}

/// Parse every `*.tf` file in `dir` into HCL bodies, in a deterministic order.
fn parse_dir(dir: &Path) -> Result<Vec<Body>> {
    let mut paths = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|source| Error::ReadDir {
        dir: dir.display().to_string(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::ReadDir {
            dir: dir.display().to_string(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("tf") {
            paths.push(path);
        }
    }
    paths.sort();

    paths
        .iter()
        .map(|path| {
            let text = std::fs::read_to_string(path).map_err(|source| Error::ReadFile {
                path: path.display().to_string(),
                source,
            })?;
            hcl::parse(&text).map_err(|source| Error::Parse {
                path: path.display().to_string(),
                source,
            })
        })
        .collect()
}

/// Load and merge `*.tfvars` files from `dir`, following Terraform's
/// precedence: `terraform.tfvars` first, then `*.auto.tfvars` in lexical order,
/// each overriding the previous (and the `variable` defaults).
fn load_tfvars(dir: &Path) -> Result<BTreeMap<String, Expression>> {
    let mut terraform = None;
    let mut auto = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|source| Error::ReadDir {
        dir: dir.display().to_string(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::ReadDir {
            dir: dir.display().to_string(),
            source,
        })?;
        let path = entry.path();
        match path.file_name().and_then(|n| n.to_str()) {
            Some("terraform.tfvars") => terraform = Some(path),
            Some(name) if name.ends_with(".auto.tfvars") => auto.push(path),
            _ => {}
        }
    }
    auto.sort();

    let ordered = terraform.into_iter().chain(auto);

    let mut vars = BTreeMap::new();
    for path in ordered {
        let text = std::fs::read_to_string(&path).map_err(|source| Error::ReadFile {
            path: path.display().to_string(),
            source,
        })?;
        let body = hcl::parse(&text).map_err(|source| Error::Parse {
            path: path.display().to_string(),
            source,
        })?;
        for (name, expr) in body_attributes(&body) {
            vars.insert(name.to_string(), expr.clone());
        }
    }
    Ok(vars)
}

/// Collect the resolvable bindings (locals, variable defaults, outputs) of a body.
fn collect_scope(body: &Body, scope: &mut ModuleScope) {
    for structure in body.iter() {
        let Some(block) = structure.as_block() else {
            continue;
        };
        match block.identifier.as_str() {
            "locals" => {
                for (key, expr) in body_attributes(&block.body) {
                    scope.locals.insert(key.to_string(), expr.clone());
                }
            }
            "variable" => {
                if let Some(name) = block.labels.first().map(|l| l.as_str())
                    && let Some(default) = attr_expr(&block.body, "default")
                {
                    scope.variables.insert(name.to_string(), default.clone());
                }
            }
            "output" => {
                if let Some(name) = block.labels.first().map(|l| l.as_str())
                    && let Some(value) = attr_expr(&block.body, "value")
                {
                    scope.outputs.insert(name.to_string(), value.clone());
                }
            }
            _ => {}
        }
    }
}

/// Collect every module call: its arguments (for `module.*.output` resolution)
/// and, when it carries `ressources` + `flows`, a glowwiththeflow instance.
fn collect_modules(body: &Body, modules: &mut Vec<FlowModule>, module_args: &mut ModuleArgs) {
    for structure in body.iter() {
        let Some(block) = structure.as_block() else {
            continue;
        };
        if block.identifier.as_str() != "module" {
            continue;
        }
        let name = block
            .labels
            .first()
            .map(|l| l.as_str().to_string())
            .unwrap_or_default();

        let args = body_attributes(&block.body)
            .filter(|(key, _)| !is_meta_argument(key))
            .map(|(key, expr)| (key.to_string(), expr.clone()))
            .collect();
        module_args.insert(name.clone(), args);

        if let (Some(ressources), Some(flows)) = (
            attr_expr(&block.body, "ressources"),
            attr_expr(&block.body, "flows"),
        ) {
            modules.push(FlowModule {
                name,
                ressources: ressources.clone(),
                flows: flows.clone(),
                vpc: attr_expr(&block.body, "vpc").cloned(),
            });
        }
    }
}

/// Terraform meta-arguments on a `module` block — not user inputs.
fn is_meta_argument(key: &str) -> bool {
    matches!(
        key,
        "source" | "version" | "count" | "for_each" | "providers" | "depends_on" | "lifecycle"
    )
}

/// Iterate the top-level attributes of a block body as `(key, expr)` pairs.
fn body_attributes(body: &Body) -> impl Iterator<Item = (&str, &Expression)> {
    body.iter()
        .filter_map(|s| s.as_attribute())
        .map(|a| (a.key.as_str(), &a.expr))
}

/// Find a single attribute by key in a block body.
fn attr_expr<'a>(body: &'a Body, key: &str) -> Option<&'a Expression> {
    body_attributes(body)
        .find(|(k, _)| *k == key)
        .map(|(_, expr)| expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_src(src: &str) -> (ModuleScope, Vec<FlowModule>, ModuleArgs) {
        let body = hcl::parse(src).expect("valid HCL");
        let mut scope = ModuleScope::default();
        let mut modules = Vec::new();
        let mut module_args = ModuleArgs::new();
        collect_scope(&body, &mut scope);
        collect_modules(&body, &mut modules, &mut module_args);
        (scope, modules, module_args)
    }

    #[test]
    fn collects_locals_variables_and_outputs() {
        let (scope, _, _) = collect_src(
            r#"
locals {
  a = ["10.0.0.0/8"]
}

variable "v" {
  default = 3
}

output "o" {
  value = local.a
}
"#,
        );
        assert!(scope.locals.contains_key("a"));
        assert!(scope.variables.contains_key("v"));
        assert!(scope.outputs.contains_key("o"));
    }

    #[test]
    fn detects_flow_module_only_with_ressources_and_flows() {
        let (_, modules, _) = collect_src(
            r#"
module "glow" {
  source     = "x"
  ressources = {}
  flows      = {}
}

module "other" {
  source = "y"
}
"#,
        );
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "glow");
    }

    #[test]
    fn captures_vpc_argument_when_present() {
        let (_, with_vpc, _) = collect_src(
            r#"
module "glow" {
  source     = "x"
  vpc        = "vpc-0abc"
  ressources = {}
  flows      = {}
}
"#,
        );
        assert!(matches!(
            with_vpc[0].vpc,
            Some(Expression::String(ref s)) if s == "vpc-0abc"
        ));

        let (_, without_vpc, _) = collect_src(
            r#"
module "glow" {
  source     = "x"
  ressources = {}
  flows      = {}
}
"#,
        );
        assert!(without_vpc[0].vpc.is_none());
    }

    #[test]
    fn captures_module_arguments_excluding_meta() {
        let (_, _, args) = collect_src(
            r#"
module "net" {
  source  = "x"
  version = "1.0"
  allowed = local.base
}
"#,
        );
        let net = args.get("net").expect("net args captured");
        assert!(net.contains_key("allowed"));
        assert!(!net.contains_key("source"));
        assert!(!net.contains_key("version"));
    }
}
