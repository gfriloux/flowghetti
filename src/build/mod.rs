//! Stage 3 — build the domain graph from resolved resources and flows.
//!
//! This is where external endpoints are deduplicated (a shared CIDR converges
//! to a single node) and where each flow becomes exactly one edge.

use crate::model::{Edge, Graph, Group, GroupKind, Node, NodeKind};
use crate::resolve::{Endpoint, FlowghettiConfig, ResolvedFlow, ResolvedResource};
use std::collections::{BTreeMap, BTreeSet};

/// The id of the zone holding every external endpoint (CIDR, prefix list,
/// unresolved reference).
const EXTERNAL_GROUP: &str = "external";

/// The group id for a VPC zone.
fn vpc_group_id(vpc: &str) -> String {
    format!("vpc_{vpc}")
}

pub fn build(
    resources: &[ResolvedResource],
    flows: &[ResolvedFlow],
    config: &FlowghettiConfig,
) -> Graph {
    let merges = resolve_merges(resources, &config.merge);
    let groups_by_origin = resolve_groups(&config.groups);

    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();

    for resource in resources {
        // A resource that merges into another contributes no node of its own —
        // the canonical resource owns the node (and its name/type/label).
        if merges.contains_key(resource.key.as_str()) {
            continue;
        }
        let id = resource_id(&resource.key);
        let label = if resource.type_.is_empty() {
            resource.name.clone()
        } else {
            format!("{}\n({})", resource.name, resource.type_)
        };
        nodes.entry(id.clone()).or_insert(Node {
            id,
            label,
            tooltip: None,
            kind: NodeKind::Resource {
                type_: resource.type_.clone(),
            },
            group: resource.vpc.as_deref().map(vpc_group_id),
        });
    }

    let mut edges = Vec::with_capacity(flows.len());
    for flow in flows {
        let from = endpoint_node(&flow.from, &merges, &groups_by_origin, &mut nodes);
        let to = endpoint_node(&flow.to, &merges, &groups_by_origin, &mut nodes);
        // A flow that becomes a self-loop once both ends are merged into the same
        // node is noise — drop it.
        if from == to {
            continue;
        }
        edges.push(Edge {
            from,
            to,
            protocol: flow.protocol.clone(),
            port: flow.port.clone(),
        });
    }

    edges.sort_by(|a, b| {
        (&a.from, &a.to)
            .cmp(&(&b.from, &b.to))
            .then_with(|| a.label().cmp(&b.label()))
    });
    edges.dedup_by(|a, b| a.from == b.from && a.to == b.to && a.label() == b.label());

    let nodes: Vec<Node> = nodes.into_values().collect();
    let groups = groups_for(resources, &nodes, &config.groups);
    Graph {
        nodes,
        edges,
        groups,
    }
}

/// Assemble the zone metadata in render order: VPC zones (sorted), then the
/// user-named zones (sorted), then the residual External zone. A zone is emitted
/// only when it has at least one member, keeping the output minimal.
fn groups_for(
    resources: &[ResolvedResource],
    nodes: &[Node],
    named: &BTreeMap<String, Vec<String>>,
) -> Vec<Group> {
    let mut groups = Vec::new();
    let used: BTreeSet<&str> = nodes.iter().filter_map(|n| n.group.as_deref()).collect();

    let vpcs: BTreeSet<&str> = resources.iter().filter_map(|r| r.vpc.as_deref()).collect();
    for vpc in vpcs {
        groups.push(Group {
            id: vpc_group_id(vpc),
            label: format!("VPC {vpc}"),
            kind: GroupKind::Vpc,
        });
    }

    // Named zones keep the membership-driven rule: a configured zone that ended
    // up claiming no endpoint is not emitted. `named` is a BTreeMap, so iteration
    // is already in sorted (zone name) order.
    for name in named.keys() {
        if used.contains(name.as_str()) {
            groups.push(Group {
                id: name.clone(),
                label: name.clone(),
                kind: GroupKind::Named,
            });
        }
    }

    if used.contains(EXTERNAL_GROUP) {
        groups.push(Group {
            id: EXTERNAL_GROUP.to_string(),
            label: "External".to_string(),
            kind: GroupKind::External,
        });
    }
    groups
}

/// Return the node id for an endpoint, materialising external nodes on demand.
/// Resource endpoints are remapped through `merges` so a flow touching a merged
/// key points at the canonical node.
fn endpoint_node(
    endpoint: &Endpoint,
    merges: &BTreeMap<String, String>,
    groups_by_origin: &BTreeMap<String, String>,
    nodes: &mut BTreeMap<String, Node>,
) -> String {
    match endpoint {
        Endpoint::Resource(key) => {
            let canonical = merges.get(key).map(String::as_str).unwrap_or(key);
            resource_id(canonical)
        }
        Endpoint::Cidrs { values, origin } => external_node(
            nodes,
            "cidr",
            values,
            origin.clone(),
            NodeKind::Cidr,
            groups_by_origin,
        ),
        Endpoint::PrefixList { values, origin } => external_node(
            nodes,
            "pl",
            values,
            origin.clone(),
            NodeKind::PrefixList,
            groups_by_origin,
        ),
        Endpoint::Symbolic { reference } => {
            let id = format!("ref:{reference}");
            let group = external_group_id(Some(reference), groups_by_origin);
            nodes.entry(id.clone()).or_insert(Node {
                id: id.clone(),
                label: reference.clone(),
                tooltip: None,
                kind: NodeKind::Unresolved,
                group: Some(group),
            });
            id
        }
    }
}

fn external_node(
    nodes: &mut BTreeMap<String, Node>,
    prefix: &str,
    values: &[String],
    origin: Option<String>,
    kind: NodeKind,
    groups_by_origin: &BTreeMap<String, String>,
) -> String {
    let id = format!("{prefix}:{}", canonical(values));
    let group = external_group_id(origin.as_deref(), groups_by_origin);
    nodes.entry(id.clone()).or_insert(Node {
        id: id.clone(),
        label: values.join("\n"),
        tooltip: origin,
        kind,
        group: Some(group),
    });
    id
}

/// The zone id for an external endpoint: its named zone when its origin
/// reference is configured in `local.flowghetti.groups`, otherwise the residual
/// `External` zone.
fn external_group_id(origin: Option<&str>, groups_by_origin: &BTreeMap<String, String>) -> String {
    origin
        .and_then(|o| groups_by_origin.get(o))
        .cloned()
        .unwrap_or_else(|| EXTERNAL_GROUP.to_string())
}

fn resource_id(key: &str) -> String {
    format!("res:{key}")
}

/// Invert the `groups` directives into an `origin reference → zone name` map.
/// An origin listed under several zones lands in the first by zone name (lexical
/// order), keeping one endpoint in exactly one zone, deterministically.
fn resolve_groups(groups: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, String> {
    let mut by_origin = BTreeMap::new();
    // `groups` is a BTreeMap, so zones are visited in sorted name order; the
    // `or_insert` then makes the first such zone win for a shared origin.
    for (name, origins) in groups {
        for origin in origins {
            by_origin
                .entry(origin.clone())
                .or_insert_with(|| name.clone());
        }
    }
    by_origin
}

/// Resolve the `merge` directives into a clean `alias key → canonical key` map,
/// keeping only entries that fold an alias into a **declared** resource.
///
/// Chains are followed transitively (`a → b → c` yields `a → c` and `b → c`).
/// An entry whose terminal is not a declared resource, or that forms a cycle, is
/// dropped with a warning — best-effort, never fatal (the alias keeps its own
/// node).
fn resolve_merges(
    resources: &[ResolvedResource],
    merge: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let declared: BTreeSet<&str> = resources.iter().map(|r| r.key.as_str()).collect();
    let mut resolved = BTreeMap::new();
    for alias in merge.keys() {
        match merge_terminal(alias, merge) {
            Ok(target) if target == alias.as_str() => {}
            Ok(target) if declared.contains(target) => {
                resolved.insert(alias.clone(), target.to_string());
            }
            Ok(target) => {
                eprintln!(
                    "flowghetti: merge target {target:?} for {alias:?} is not a declared resource — ignoring"
                );
            }
            Err(()) => {
                eprintln!("flowghetti: merge cycle involving {alias:?} — ignoring");
            }
        }
    }
    resolved
}

/// Follow the merge chain from `key` to its terminal — the first key that is not
/// itself an alias. `Err` on a cycle.
fn merge_terminal<'a>(key: &'a str, merge: &'a BTreeMap<String, String>) -> Result<&'a str, ()> {
    let mut seen = BTreeSet::new();
    seen.insert(key);
    let mut current = key;
    while let Some(next) = merge.get(current) {
        if !seen.insert(next.as_str()) {
            return Err(());
        }
        current = next.as_str();
    }
    Ok(current)
}

/// Order-independent identity of a value set, for deduplication.
fn canonical(values: &[String]) -> String {
    let mut sorted: Vec<&str> = values.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::PortSpec;

    fn resource(key: &str) -> ResolvedResource {
        ResolvedResource {
            key: key.to_string(),
            name: key.to_string(),
            type_: "ec2".to_string(),
            vpc: None,
        }
    }

    fn cidr(value: &str) -> Endpoint {
        Endpoint::Cidrs {
            values: vec![value.to_string()],
            origin: None,
        }
    }

    #[test]
    fn canonical_is_order_independent() {
        assert_eq!(
            canonical(&["b/32".to_string(), "a/32".to_string()]),
            canonical(&["a/32".to_string(), "b/32".to_string()]),
        );
    }

    #[test]
    fn shared_external_endpoint_is_deduplicated() {
        let resources = [resource("app")];
        let flows = [
            ResolvedFlow {
                name: "https".to_string(),
                from: cidr("0.0.0.0/0"),
                to: Endpoint::Resource("app".to_string()),
                port: PortSpec::Single(443),
                protocol: "tcp".to_string(),
            },
            ResolvedFlow {
                name: "http".to_string(),
                from: cidr("0.0.0.0/0"),
                to: Endpoint::Resource("app".to_string()),
                port: PortSpec::Single(80),
                protocol: "tcp".to_string(),
            },
        ];

        let graph = build(&resources, &flows, &FlowghettiConfig::default());

        // one resource node + one shared external node
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 2);
        assert_eq!(graph.edges[0].from, graph.edges[1].from);
    }

    #[test]
    fn identical_flows_collapse_to_one_edge() {
        let resources = [resource("a"), resource("b")];
        let flow = || ResolvedFlow {
            name: "ssh".to_string(),
            from: Endpoint::Resource("a".to_string()),
            to: Endpoint::Resource("b".to_string()),
            port: PortSpec::Single(22),
            protocol: "tcp".to_string(),
        };

        let graph = build(&resources, &[flow(), flow()], &FlowghettiConfig::default());

        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].label(), "tcp/22");
    }

    #[test]
    fn external_endpoints_join_the_external_group() {
        let resources = [resource("app")];
        let flows = [
            ResolvedFlow {
                name: "https".to_string(),
                from: cidr("0.0.0.0/0"),
                to: Endpoint::Resource("app".to_string()),
                port: PortSpec::Single(443),
                protocol: "tcp".to_string(),
            },
            ResolvedFlow {
                name: "ref".to_string(),
                from: Endpoint::Symbolic {
                    reference: "data.aws_x.y.id".to_string(),
                },
                to: Endpoint::Resource("app".to_string()),
                port: PortSpec::Any,
                protocol: "tcp".to_string(),
            },
        ];

        let graph = build(&resources, &flows, &FlowghettiConfig::default());

        // the resource stays ungrouped; the CIDR and the unresolved ref both
        // land in the single "external" zone.
        let group_of = |id: &str| {
            graph
                .nodes
                .iter()
                .find(|n| n.id == id)
                .and_then(|n| n.group.clone())
        };
        assert_eq!(group_of("res:app"), None);
        assert_eq!(group_of("cidr:0.0.0.0/0").as_deref(), Some(EXTERNAL_GROUP));
        assert_eq!(
            group_of("ref:data.aws_x.y.id").as_deref(),
            Some(EXTERNAL_GROUP)
        );
        // exactly one external zone is emitted, since it has members.
        assert_eq!(graph.groups.len(), 1);
        assert_eq!(graph.groups[0].id, EXTERNAL_GROUP);
    }

    #[test]
    fn resources_join_their_vpc_group() {
        let in_vpc = |key: &str, vpc: &str| ResolvedResource {
            key: key.to_string(),
            name: key.to_string(),
            type_: "ec2".to_string(),
            vpc: Some(vpc.to_string()),
        };
        let resources = [in_vpc("a", "vpc-1"), in_vpc("b", "vpc-1")];

        let graph = build(&resources, &[], &FlowghettiConfig::default());

        // both resources share the one VPC zone
        assert_eq!(graph.groups.len(), 1);
        assert_eq!(graph.groups[0].id, "vpc_vpc-1");
        assert_eq!(graph.groups[0].label, "VPC vpc-1");
        assert!(
            graph
                .nodes
                .iter()
                .all(|n| n.group.as_deref() == Some("vpc_vpc-1"))
        );
    }

    #[test]
    fn no_group_emitted_without_external_endpoints() {
        let resources = [resource("a"), resource("b")];
        let flows = [ResolvedFlow {
            name: "ssh".to_string(),
            from: Endpoint::Resource("a".to_string()),
            to: Endpoint::Resource("b".to_string()),
            port: PortSpec::Single(22),
            protocol: "tcp".to_string(),
        }];

        let graph = build(&resources, &flows, &FlowghettiConfig::default());

        assert!(graph.groups.is_empty());
        assert!(graph.nodes.iter().all(|n| n.group.is_none()));
    }

    fn merge_config(pairs: &[(&str, &str)]) -> FlowghettiConfig {
        FlowghettiConfig {
            merge: pairs
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            ..FlowghettiConfig::default()
        }
    }

    fn flow(from: &str, to: &str, port: i64) -> ResolvedFlow {
        ResolvedFlow {
            name: format!("{from}_{to}"),
            from: Endpoint::Resource(from.to_string()),
            to: Endpoint::Resource(to.to_string()),
            port: PortSpec::Single(port),
            protocol: "tcp".to_string(),
        }
    }

    #[test]
    fn merge_folds_aliases_into_one_node_and_remaps_flows() {
        let resources = [resource("canon"), resource("alias"), resource("peer")];
        let flows = [
            // remapped: alias -> peer becomes canon -> peer
            flow("alias", "peer", 443),
            // becomes canon -> canon, a self-loop → dropped
            flow("alias", "canon", 22),
        ];

        let graph = build(&resources, &flows, &merge_config(&[("alias", "canon")]));

        // the alias contributes no node; the canonical one stays
        assert!(graph.nodes.iter().any(|n| n.id == "res:canon"));
        assert!(!graph.nodes.iter().any(|n| n.id == "res:alias"));
        // exactly one edge remains, remapped onto the canonical node; no self-loop
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].from, "res:canon");
        assert_eq!(graph.edges[0].to, "res:peer");
        assert!(graph.edges.iter().all(|e| e.from != e.to));
    }

    #[test]
    fn merge_chains_resolve_transitively() {
        let resources = [
            resource("a"),
            resource("b"),
            resource("c"),
            resource("peer"),
        ];
        let flows = [flow("a", "peer", 443)];

        let graph = build(&resources, &flows, &merge_config(&[("a", "b"), ("b", "c")]));

        // a and b both fold into c
        assert!(graph.nodes.iter().any(|n| n.id == "res:c"));
        assert!(!graph.nodes.iter().any(|n| n.id == "res:a"));
        assert!(!graph.nodes.iter().any(|n| n.id == "res:b"));
        // the flow from a now originates at c
        assert_eq!(graph.edges[0].from, "res:c");
    }

    #[test]
    fn merge_into_undeclared_target_is_ignored() {
        let resources = [resource("alias")];

        let graph = build(&resources, &[], &merge_config(&[("alias", "ghost")]));

        // ghost is not a declared resource → the alias keeps its own node
        assert!(graph.nodes.iter().any(|n| n.id == "res:alias"));
        assert!(!graph.nodes.iter().any(|n| n.id == "res:ghost"));
    }

    #[test]
    fn merge_cycle_is_ignored() {
        let resources = [resource("a"), resource("b")];

        // a → b → a is a cycle: both entries are dropped, both nodes survive
        let graph = build(&resources, &[], &merge_config(&[("a", "b"), ("b", "a")]));

        assert_eq!(graph.nodes.len(), 2);
        assert!(graph.nodes.iter().any(|n| n.id == "res:a"));
        assert!(graph.nodes.iter().any(|n| n.id == "res:b"));
    }

    fn groups_config(pairs: &[(&str, &[&str])]) -> FlowghettiConfig {
        FlowghettiConfig {
            groups: pairs
                .iter()
                .map(|(name, origins)| {
                    (
                        name.to_string(),
                        origins.iter().map(|s| s.to_string()).collect(),
                    )
                })
                .collect(),
            ..FlowghettiConfig::default()
        }
    }

    fn cidr_from(value: &str, origin: &str) -> Endpoint {
        Endpoint::Cidrs {
            values: vec![value.to_string()],
            origin: Some(origin.to_string()),
        }
    }

    fn to_app(from: Endpoint) -> ResolvedFlow {
        ResolvedFlow {
            name: "f".to_string(),
            from,
            to: Endpoint::Resource("app".to_string()),
            port: PortSpec::Single(443),
            protocol: "tcp".to_string(),
        }
    }

    fn group_of<'a>(graph: &'a Graph, id: &str) -> Option<&'a str> {
        graph
            .nodes
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| n.group.as_deref())
    }

    #[test]
    fn named_groups_route_endpoints_by_origin_and_keep_external_residual() {
        let resources = [resource("app")];
        let flows = [
            to_app(cidr_from("198.51.100.0/24", "local.office")),
            to_app(cidr("0.0.0.0/0")), // no origin → residual External
        ];

        let graph = build(
            &resources,
            &flows,
            &groups_config(&[("Office", &["local.office"])]),
        );

        assert_eq!(group_of(&graph, "cidr:198.51.100.0/24"), Some("Office"));
        assert_eq!(group_of(&graph, "cidr:0.0.0.0/0"), Some("external"));
        // the Office zone is emitted as a Named group, ahead of External
        let office = graph.groups.iter().find(|g| g.id == "Office").unwrap();
        assert!(matches!(office.kind, GroupKind::Named));
        let order: Vec<&str> = graph.groups.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(order, vec!["Office", "external"]);
    }

    #[test]
    fn shared_origin_lands_in_first_lexical_zone() {
        let resources = [resource("app")];
        let flows = [to_app(cidr_from("198.51.100.0/24", "local.shared"))];

        // both zones claim the same origin; "Alpha" (lexically first) wins.
        let graph = build(
            &resources,
            &flows,
            &groups_config(&[("Beta", &["local.shared"]), ("Alpha", &["local.shared"])]),
        );

        assert_eq!(group_of(&graph, "cidr:198.51.100.0/24"), Some("Alpha"));
    }
}
