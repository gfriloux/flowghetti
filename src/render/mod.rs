//! Stage 4 — render the domain graph as Graphviz DOT.
//!
//! The only stage that knows DOT syntax. Output is deterministic so it can be
//! compared against golden files.

use crate::model::{Edge, Graph, Group, GroupKind, Node, NodeKind, PortSpec};

/// Which named palette to render with. Selected at the CLI; carries no clap
/// dependency so `render` stays independent of the interface layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeChoice {
    #[default]
    Light,
    Dark,
}

pub fn to_dot(
    graph: &Graph,
    rankdir: &str,
    choice: ThemeChoice,
    legend: bool,
    title: Option<&str>,
) -> String {
    let theme = match choice {
        ThemeChoice::Light => Theme::light(),
        ThemeChoice::Dark => Theme::dark(),
    };

    let mut out = String::new();
    out.push_str("digraph flows {\n");
    out.push_str(&format!("  rankdir={rankdir};\n"));
    if let Some(bg) = theme.bg {
        out.push_str(&format!("  bgcolor={};\n", quote(bg)));
    }
    if let Some(title) = title {
        out.push_str(&format!("  label={};\n", quote(title)));
        out.push_str("  labelloc=t;\n");
        out.push_str("  fontname=\"sans-serif\";\n");
        out.push_str("  fontsize=16;\n");
        if let Some(font) = theme.node_font {
            out.push_str(&format!("  fontcolor={};\n", quote(font)));
        }
    }

    let mut node_defaults =
        String::from("fontname=\"sans-serif\", penwidth=1.6, gradientangle=270");
    if let Some(font) = theme.node_font {
        node_defaults.push_str(&format!(", fontcolor={}", quote(font)));
    }
    out.push_str(&format!("  node [{node_defaults}];\n"));
    out.push_str("  edge [fontname=\"sans-serif\", penwidth=1.4];\n");

    for group in &graph.groups {
        out.push_str(&render_group(group, graph, &theme));
    }
    for node in &graph.nodes {
        if node.group.is_none() {
            out.push_str(&render_node(node, &theme));
        }
    }
    for edge in &graph.edges {
        out.push_str(&render_edge(edge, &theme));
    }
    if legend {
        out.push_str(&render_legend(&theme));
    }

    out.push_str("}\n");
    out
}

fn render_node(node: &Node, theme: &Theme) -> String {
    node_line(node, theme, "  ")
}

/// A single node declaration line, at the given indent. Shared by top-level
/// nodes (`render_node`) and nodes nested inside a group cluster.
fn node_line(node: &Node, theme: &Theme, indent: &str) -> String {
    let style = theme.style(&node.kind);
    let attrs = node_attrs(&node.label, &style, node.tooltip.as_deref());
    format!("{indent}{} [{attrs}];\n", quote(&node.id))
}

/// Render a zone as a `subgraph cluster_*`, wrapping its member nodes. Styled
/// from the active theme's palette for the group's kind. Member nodes are taken
/// from `graph.nodes` (already sorted) filtered by membership.
fn render_group(group: &Group, graph: &Graph, theme: &Theme) -> String {
    let zone = theme.group_style(&group.kind);
    let mut out = format!(
        "  subgraph {} {{\n",
        quote(&format!("cluster_{}", group.id))
    );
    out.push_str(&format!("    label={};\n", quote(&group.label)));
    out.push_str("    labelloc=t;\n");
    out.push_str("    fontsize=12;\n");
    out.push_str("    style=\"rounded,filled\";\n");
    out.push_str(&format!("    fillcolor={};\n", quote(zone.fill)));
    out.push_str(&format!("    color={};\n", quote(zone.border)));
    out.push_str(&format!("    fontcolor={};\n", quote(zone.font)));
    for node in &graph.nodes {
        if node.group.as_deref() == Some(group.id.as_str()) {
            out.push_str(&node_line(node, theme, "    "));
        }
    }
    out.push_str("  }\n");
    out
}

/// Build the bracketed attribute list for a node from its label, style and an
/// optional tooltip. Shared by [`render_node`] and the legend.
fn node_attrs(label: &str, style: &NodeStyle, tooltip: Option<&str>) -> String {
    let mut style_words = vec!["filled"];
    if style.rounded {
        style_words.push("rounded");
    }
    if style.dashed {
        style_words.push("dashed");
    }
    let mut attrs = format!(
        "label={}, shape={}, style=\"{}\", fillcolor={}, color={}",
        quote(label),
        style.shape,
        style_words.join(","),
        quote(style.fill),
        quote(style.border),
    );
    if let Some(tooltip) = tooltip {
        attrs.push_str(&format!(", tooltip={}", quote(tooltip)));
    }
    attrs
}

fn render_edge(edge: &Edge, theme: &Theme) -> String {
    let style = theme.edge_style(classify_edge(&edge.protocol, &edge.port));
    let mut attrs = format!(
        "label={}, color={}, fontcolor={}",
        quote(&edge.label()),
        quote(style.color),
        quote(style.color),
    );
    let mut style_words = Vec::new();
    if style.bold {
        style_words.push("bold");
    }
    if style.dashed {
        style_words.push("dashed");
    }
    if !style_words.is_empty() {
        attrs.push_str(&format!(", style=\"{}\"", style_words.join(",")));
    }
    if style.thick {
        attrs.push_str(", penwidth=2.2");
    }
    format!(
        "  {} -> {} [{attrs}];\n",
        quote(&edge.from),
        quote(&edge.to),
    )
}

/// Render a legend cluster documenting the node categories, styled in the active
/// theme's palette. One representative node per category, kept on a row by an
/// invisible chain. Opt-in (`--legend`).
fn render_legend(theme: &Theme) -> String {
    let entries: [(&str, &str, NodeKind); 7] = [
        ("lg_compute", "compute (ec2/ecs…)", resource("ec2")),
        ("lg_lb", "load balancer", resource("nlb")),
        ("lg_db", "database", resource("rds")),
        ("lg_serverless", "serverless (lambda)", resource("lambda")),
        ("lg_cidr", "CIDR / endpoint", NodeKind::Cidr),
        ("lg_prefixlist", "prefix list", NodeKind::PrefixList),
        ("lg_unresolved", "unresolved ref", NodeKind::Unresolved),
    ];

    let mut out = String::new();
    out.push_str("  subgraph cluster_legend {\n");
    out.push_str("    label=\"Legend\";\n");
    out.push_str("    labelloc=t;\n");
    out.push_str("    fontsize=12;\n");
    out.push_str("    style=\"rounded\";\n");
    out.push_str("    color=\"#999999\";\n");
    if let Some(font) = theme.node_font {
        out.push_str(&format!("    fontcolor={};\n", quote(font)));
    }
    out.push_str("    node [fontsize=10];\n");

    for (id, label, kind) in &entries {
        let attrs = node_attrs(label, &theme.style(kind), None);
        out.push_str(&format!("    {} [{attrs}];\n", quote(id)));
    }

    let chain = entries
        .iter()
        .map(|(id, _, _)| quote(id))
        .collect::<Vec<_>>()
        .join(" -> ");
    out.push_str(&format!("    {chain} [style=invis];\n"));
    out.push_str("  }\n");
    out
}

/// Helper: a `NodeKind::Resource` of the given type, for legend entries.
fn resource(type_: &str) -> NodeKind {
    NodeKind::Resource {
        type_: type_.to_string(),
    }
}

/// The visual style applied to a node.
///
/// `shape`, `rounded` and `dashed` are **structural** — they encode the node's
/// category and never change between themes. `fill` and `border` are the
/// **palette**, supplied by the active [`Theme`] (`fill` may be a `a:b` gradient).
struct NodeStyle {
    shape: &'static str,
    fill: &'static str,
    border: &'static str,
    dashed: bool,
    rounded: bool,
}

/// The semantic category of an internal resource, derived from its (free-form)
/// AWS type. Purely about *what* a resource is — carries no colour. A [`Theme`]
/// turns a category into a concrete [`NodeStyle`].
enum ResourceCategory {
    LoadBalancer,
    Database,
    Serverless,
    Compute,
    Other,
}

/// Classify a free-form AWS type into a [`ResourceCategory`]. Theme-independent.
fn classify(type_: &str) -> ResourceCategory {
    const LB: &[&str] = &["nlb", "alb", "elb", "clb", "gwlb", "lb", "loadbalancer"];
    const DB: &[&str] = &[
        "rds",
        "aurora",
        "db",
        "database",
        "dynamodb",
        "elasticache",
        "redis",
        "memcached",
        "postgres",
        "postgresql",
        "mysql",
        "mariadb",
    ];
    const SERVERLESS: &[&str] = &["lambda", "function"];
    const COMPUTE: &[&str] = &[
        "ec2",
        "instance",
        "vm",
        "ecs",
        "fargate",
        "eks",
        "container",
        "node",
    ];

    let t = type_.to_ascii_lowercase();
    if LB.contains(&t.as_str()) {
        ResourceCategory::LoadBalancer
    } else if DB.contains(&t.as_str()) {
        ResourceCategory::Database
    } else if SERVERLESS.contains(&t.as_str()) {
        ResourceCategory::Serverless
    } else if COMPUTE.contains(&t.as_str()) {
        ResourceCategory::Compute
    } else {
        ResourceCategory::Other
    }
}

/// The visual style applied to an edge. `color` is the palette (theme-supplied);
/// `bold`, `dashed` and `thick` are structural emphasis tied to the category.
struct EdgeStyle {
    color: &'static str,
    bold: bool,
    dashed: bool,
    thick: bool,
}

/// The semantic category of a flow, derived from its protocol and port. Purely
/// about *what kind* of flow it is — carries no colour. A [`Theme`] turns a
/// category into a concrete [`EdgeStyle`].
enum EdgeCategory {
    /// Secure web (HTTPS).
    Secure,
    /// Administrative access (SSH, RDP).
    Admin,
    /// Database / cache ports.
    Database,
    /// A fully-permissive `all` rule — worth flagging loudly.
    Permissive,
    /// Anything else (plain HTTP, app ports, ranges, unknown).
    Neutral,
}

/// Classify a flow into an [`EdgeCategory`]. Theme-independent; unknown
/// protocol/port combinations degrade to [`EdgeCategory::Neutral`].
fn classify_edge(protocol: &str, port: &PortSpec) -> EdgeCategory {
    if protocol == "all" {
        return EdgeCategory::Permissive;
    }
    match port {
        PortSpec::Single(443 | 8443) => EdgeCategory::Secure,
        PortSpec::Single(22 | 3389) => EdgeCategory::Admin,
        PortSpec::Single(5432 | 3306 | 1433 | 1521 | 6379 | 11211 | 27017 | 9200 | 5601) => {
            EdgeCategory::Database
        }
        _ => EdgeCategory::Neutral,
    }
}

/// A fill + border pair for one node slot. `fill` may be a `a:b` gradient.
struct Paint {
    fill: &'static str,
    border: &'static str,
}

/// The palette for a zone cluster: a flat `fill`, a `border` colour, and the
/// `font` colour of the cluster label.
struct ZonePaint {
    fill: &'static str,
    border: &'static str,
    font: &'static str,
}

/// A named palette: maps node categories to a [`Paint`]. Shapes, rounding and
/// the dashed marker are structural and live in the style methods, not here.
///
/// `bg` and `node_font` are emitted only when `Some` — that keeps the light
/// theme's output identical to flowghetti's pre-theme rendering (no `bgcolor`,
/// default black node text), while dark themes opt into a dark canvas and light
/// node text.
struct Theme {
    bg: Option<&'static str>,
    node_font: Option<&'static str>,
    lb: Paint,
    db: Paint,
    serverless: Paint,
    compute: Paint,
    resource_other: Paint,
    cidr: Paint,
    prefix_list: Paint,
    unresolved: Paint,
    zone_vpc: ZonePaint,
    zone_external: ZonePaint,
    zone_named: ZonePaint,
    edge_secure: &'static str,
    edge_admin: &'static str,
    edge_database: &'static str,
    edge_permissive: &'static str,
    edge_neutral: &'static str,
}

impl Theme {
    /// The default light palette: soft vertical gradients with a saturated
    /// border per category, on the default (white) canvas with black text.
    fn light() -> Self {
        Theme {
            bg: None,
            node_font: None,
            lb: Paint {
                fill: "#e2f7e2:#bfeec0",
                border: "#2e9e4f",
            },
            db: Paint {
                fill: "#fff0d6:#ffd699",
                border: "#d98a1f",
            },
            serverless: Paint {
                fill: "#f6e6ff:#e9ccff",
                border: "#9b51e0",
            },
            compute: Paint {
                fill: "#eaf3ff:#cfe8ff",
                border: "#3b7dd8",
            },
            resource_other: Paint {
                fill: "#f5f5f5:#e0e0e0",
                border: "#9aa0a6",
            },
            cidr: Paint {
                fill: "#f7f7f7",
                border: "#9aa0a6",
            },
            prefix_list: Paint {
                fill: "#eee9fb",
                border: "#8a6dd6",
            },
            unresolved: Paint {
                fill: "#fff3cd",
                border: "#d9a300",
            },
            zone_vpc: ZonePaint {
                fill: "#eef4ff",
                border: "#3b7dd8",
                font: "#2e6fd8",
            },
            zone_external: ZonePaint {
                fill: "#f0f0f0",
                border: "#9aa0a6",
                font: "#5f6b7a",
            },
            // Named zones share one teal palette, distinct from VPC (blue) and
            // External (grey).
            zone_named: ZonePaint {
                fill: "#e6f5f3",
                border: "#2aa39a",
                font: "#1f857d",
            },
            edge_secure: "#2e6fd8",
            edge_admin: "#d98a1f",
            edge_database: "#2e9e4f",
            edge_permissive: "#d64545",
            edge_neutral: "#5f6b7a",
        }
    }

    /// A dark palette: deep flat fills with luminous borders on a dark canvas,
    /// light node text, and brighter edge colours that read on a dark
    /// background. Same shapes, rounding and emphasis as [`Theme::light`].
    fn dark() -> Self {
        Theme {
            bg: Some("#1e1f24"),
            node_font: Some("#f0f0f0"),
            lb: Paint {
                fill: "#1f3d2a",
                border: "#4ec97a",
            },
            db: Paint {
                fill: "#4a3413",
                border: "#e0a23f",
            },
            serverless: Paint {
                fill: "#33214a",
                border: "#b888f0",
            },
            compute: Paint {
                fill: "#1d3147",
                border: "#5ba2e8",
            },
            resource_other: Paint {
                fill: "#2a2b30",
                border: "#8a8f98",
            },
            cidr: Paint {
                fill: "#2a2b30",
                border: "#8a8f98",
            },
            prefix_list: Paint {
                fill: "#2c2740",
                border: "#a98fe0",
            },
            unresolved: Paint {
                fill: "#3a3416",
                border: "#e0c14a",
            },
            zone_vpc: ZonePaint {
                fill: "#1d2a3d",
                border: "#5ba2e8",
                font: "#a9c8ef",
            },
            zone_external: ZonePaint {
                fill: "#26272c",
                border: "#8a8f98",
                font: "#c9ced6",
            },
            zone_named: ZonePaint {
                fill: "#15302c",
                border: "#3fb3a8",
                font: "#86d3ca",
            },
            edge_secure: "#5ba2e8",
            edge_admin: "#e0a23f",
            edge_database: "#4ec97a",
            edge_permissive: "#e86060",
            edge_neutral: "#9aa4b2",
        }
    }

    /// The full style for a node: its structural shape/rounding/dashing plus
    /// this theme's fill and border.
    fn style(&self, kind: &NodeKind) -> NodeStyle {
        match kind {
            NodeKind::Resource { type_ } => self.resource_style(classify(type_)),
            NodeKind::Cidr => NodeStyle {
                shape: "note",
                fill: self.cidr.fill,
                border: self.cidr.border,
                dashed: false,
                rounded: false,
            },
            NodeKind::PrefixList => NodeStyle {
                shape: "note",
                fill: self.prefix_list.fill,
                border: self.prefix_list.border,
                dashed: false,
                rounded: false,
            },
            NodeKind::Unresolved => NodeStyle {
                shape: "note",
                fill: self.unresolved.fill,
                border: self.unresolved.border,
                dashed: true,
                rounded: false,
            },
        }
    }

    fn resource_style(&self, category: ResourceCategory) -> NodeStyle {
        match category {
            ResourceCategory::LoadBalancer => NodeStyle {
                shape: "ellipse",
                fill: self.lb.fill,
                border: self.lb.border,
                dashed: false,
                rounded: false,
            },
            ResourceCategory::Database => NodeStyle {
                shape: "cylinder",
                fill: self.db.fill,
                border: self.db.border,
                dashed: false,
                rounded: false,
            },
            ResourceCategory::Serverless => NodeStyle {
                shape: "component",
                fill: self.serverless.fill,
                border: self.serverless.border,
                dashed: false,
                rounded: false,
            },
            // Box-shaped resources get rounded corners.
            ResourceCategory::Compute => NodeStyle {
                shape: "box",
                fill: self.compute.fill,
                border: self.compute.border,
                dashed: false,
                rounded: true,
            },
            ResourceCategory::Other => NodeStyle {
                shape: "box",
                fill: self.resource_other.fill,
                border: self.resource_other.border,
                dashed: false,
                rounded: true,
            },
        }
    }

    /// This theme's palette for a zone cluster, keyed by the group's kind.
    fn group_style(&self, kind: &GroupKind) -> &ZonePaint {
        match kind {
            GroupKind::Vpc => &self.zone_vpc,
            GroupKind::External => &self.zone_external,
            GroupKind::Named => &self.zone_named,
        }
    }

    /// The full style for an edge: this theme's colour plus the category's
    /// structural emphasis.
    fn edge_style(&self, category: EdgeCategory) -> EdgeStyle {
        match category {
            EdgeCategory::Secure => EdgeStyle {
                color: self.edge_secure,
                bold: false,
                dashed: false,
                thick: true,
            },
            EdgeCategory::Admin => EdgeStyle {
                color: self.edge_admin,
                bold: false,
                dashed: false,
                thick: false,
            },
            EdgeCategory::Database => EdgeStyle {
                color: self.edge_database,
                bold: false,
                dashed: false,
                thick: false,
            },
            // A permissive `all` rule is flagged loudly: bold red, dashed, thick.
            EdgeCategory::Permissive => EdgeStyle {
                color: self.edge_permissive,
                bold: true,
                dashed: true,
                thick: true,
            },
            EdgeCategory::Neutral => EdgeStyle {
                color: self.edge_neutral,
                bold: false,
                dashed: false,
                thick: false,
            },
        }
    }
}

/// Quote and escape a string as a DOT identifier/label.
fn quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_escapes_specials() {
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote("a\nb"), "\"a\\nb\"");
        assert_eq!(quote("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn renders_rankdir_shapes_and_tooltip() {
        let graph = Graph {
            nodes: vec![
                Node {
                    id: "res:app".to_string(),
                    label: "app\n(ec2)".to_string(),
                    tooltip: None,
                    kind: NodeKind::Resource {
                        type_: "ec2".to_string(),
                    },
                    group: None,
                },
                Node {
                    id: "cidr:x".to_string(),
                    label: "203.0.113.0/24".to_string(),
                    tooltip: Some("local.office".to_string()),
                    kind: NodeKind::Cidr,
                    group: None,
                },
            ],
            edges: vec![Edge {
                from: "cidr:x".to_string(),
                to: "res:app".to_string(),
                protocol: "tcp".to_string(),
                port: PortSpec::Single(443),
            }],
            groups: Vec::new(),
        };

        let dot = to_dot(&graph, "TB", ThemeChoice::Light, false, None);

        assert!(dot.contains("rankdir=TB;"));
        // no legend or title unless requested
        assert!(!dot.contains("cluster_legend"));
        assert!(!dot.contains("labelloc=t"));
        // light theme adds no canvas / node-font overrides
        assert!(!dot.contains("bgcolor"));
        assert!(
            !dot.contains(
                "node [fontname=\"sans-serif\", penwidth=1.6, gradientangle=270, fontcolor"
            )
        );
        assert!(dot.contains("shape=box")); // ec2 resource
        assert!(dot.contains("shape=note")); // cidr endpoint
        assert!(dot.contains("tooltip=\"local.office\""));
        // tcp/443 is classified as a secure flow: coloured + thickened.
        assert!(dot.contains(
            "\"cidr:x\" -> \"res:app\" [label=\"tcp/443\", color=\"#2e6fd8\", fontcolor=\"#2e6fd8\", penwidth=2.2];"
        ));
    }

    fn node(kind: NodeKind) -> String {
        render_node(
            &Node {
                id: "n".to_string(),
                label: "n".to_string(),
                tooltip: None,
                kind,
                group: None,
            },
            &Theme::light(),
        )
    }

    fn edge(protocol: &str, port: PortSpec) -> String {
        render_edge(
            &Edge {
                from: "a".to_string(),
                to: "b".to_string(),
                protocol: protocol.to_string(),
                port,
            },
            &Theme::light(),
        )
    }

    #[test]
    fn colours_edges_by_protocol_and_port() {
        // secure web: blue + thick
        let secure = edge("tcp", PortSpec::Single(443));
        assert!(secure.contains("color=\"#2e6fd8\""));
        assert!(secure.contains("penwidth=2.2"));
        // database port: green
        assert!(edge("tcp", PortSpec::Single(5432)).contains("color=\"#2e9e4f\""));
        // admin: orange
        assert!(edge("tcp", PortSpec::Single(22)).contains("color=\"#d98a1f\""));
        // permissive `all`: red, bold + dashed
        let permissive = edge("all", PortSpec::Single(0));
        assert!(permissive.contains("color=\"#d64545\""));
        assert!(permissive.contains("style=\"bold,dashed\""));
        // unknown port degrades to the neutral colour, no emphasis
        let neutral = edge("tcp", PortSpec::Single(8080));
        assert!(neutral.contains("color=\"#5f6b7a\""));
        assert!(!neutral.contains("penwidth"));
        assert!(!neutral.contains("style="));
    }

    #[test]
    fn external_group_renders_as_a_cluster_wrapping_its_members() {
        let graph = Graph {
            nodes: vec![
                Node {
                    id: "cidr:x".to_string(),
                    label: "203.0.113.0/24".to_string(),
                    tooltip: None,
                    kind: NodeKind::Cidr,
                    group: Some("external".to_string()),
                },
                Node {
                    id: "res:app".to_string(),
                    label: "app".to_string(),
                    tooltip: None,
                    kind: NodeKind::Resource {
                        type_: "ec2".to_string(),
                    },
                    group: None,
                },
            ],
            edges: Vec::new(),
            groups: vec![Group {
                id: "external".to_string(),
                label: "External".to_string(),
                kind: GroupKind::External,
            }],
        };

        let dot = to_dot(&graph, "LR", ThemeChoice::Light, false, None);

        // the cluster is emitted, themed, and labelled
        assert!(dot.contains("subgraph \"cluster_external\" {"));
        assert!(dot.contains("label=\"External\";"));
        assert!(dot.contains("style=\"rounded,filled\";"));
        assert!(dot.contains("fillcolor=\"#f0f0f0\";"));
        // the member node is nested (4-space indent), the ungrouped one is not
        assert!(dot.contains("    \"cidr:x\" ["));
        assert!(dot.contains("  \"res:app\" ["));
        assert!(!dot.contains("    \"res:app\" ["));
    }

    #[test]
    fn dark_theme_sets_canvas_and_dark_palette() {
        let graph = Graph {
            nodes: vec![Node {
                id: "res:db".to_string(),
                label: "db".to_string(),
                tooltip: None,
                kind: NodeKind::Resource {
                    type_: "rds".to_string(),
                },
                group: None,
            }],
            edges: vec![Edge {
                from: "res:db".to_string(),
                to: "res:db".to_string(),
                protocol: "tcp".to_string(),
                port: PortSpec::Single(443),
            }],
            groups: Vec::new(),
        };

        let dot = to_dot(&graph, "LR", ThemeChoice::Dark, false, None);

        // dark canvas + light node text
        assert!(dot.contains("bgcolor=\"#1e1f24\""));
        assert!(dot.contains("fontcolor=\"#f0f0f0\""));
        // dark node fill (rds) and dark edge colour (secure)
        assert!(dot.contains("fillcolor=\"#4a3413\""));
        assert!(dot.contains("color=\"#5ba2e8\""));
    }

    #[test]
    fn legend_is_opt_in_and_themed() {
        let graph = Graph {
            nodes: Vec::new(),
            edges: Vec::new(),
            groups: Vec::new(),
        };

        // off by default
        assert!(!to_dot(&graph, "LR", ThemeChoice::Light, false, None).contains("cluster_legend"));

        // on request: a legend cluster with one entry per category
        let light = to_dot(&graph, "LR", ThemeChoice::Light, true, None);
        assert!(light.contains("subgraph cluster_legend"));
        assert!(light.contains("\"lg_compute\""));
        assert!(light.contains("\"lg_unresolved\""));
        assert!(light.contains("[style=invis]"));
        // legend swatches use the active theme's palette (dark fill here)
        let dark = to_dot(&graph, "LR", ThemeChoice::Dark, true, None);
        assert!(dark.contains("\"lg_db\" [label=\"database\", shape=cylinder"));
        assert!(dark.contains("fillcolor=\"#4a3413\""));
    }

    #[test]
    fn title_is_opt_in_and_themed() {
        let graph = Graph {
            nodes: Vec::new(),
            edges: Vec::new(),
            groups: Vec::new(),
        };

        // off by default: no graph-level label
        assert!(!to_dot(&graph, "LR", ThemeChoice::Light, false, None).contains("labelloc=t"));

        // on request: a top-anchored graph title
        let titled = to_dot(&graph, "LR", ThemeChoice::Light, false, Some("staging"));
        assert!(titled.contains("label=\"staging\";"));
        assert!(titled.contains("labelloc=t;"));

        // dark gives the title a light font colour
        let dark = to_dot(&graph, "LR", ThemeChoice::Dark, false, Some("staging"));
        assert!(dark.contains("fontcolor=\"#f0f0f0\""));
    }

    #[test]
    fn styles_resources_by_category() {
        let compute = node(NodeKind::Resource {
            type_: "ec2".into(),
        });
        assert!(compute.contains("shape=box"));
        // compute boxes are rounded and get a coloured border + gradient fill
        assert!(compute.contains("style=\"filled,rounded\""));
        assert!(compute.contains("color=\"#3b7dd8\""));
        assert!(compute.contains("fillcolor=\"#eaf3ff:#cfe8ff\""));
        assert!(
            node(NodeKind::Resource {
                type_: "nlb".into()
            })
            .contains("shape=ellipse")
        );
        assert!(
            node(NodeKind::Resource {
                type_: "rds".into()
            })
            .contains("shape=cylinder")
        );
        assert!(
            node(NodeKind::Resource {
                type_: "lambda".into()
            })
            .contains("shape=component")
        );
        // unknown type falls back to a neutral grey box
        let unknown = node(NodeKind::Resource {
            type_: "weird".into(),
        });
        assert!(unknown.contains("shape=box"));
        assert!(unknown.contains("fillcolor=\"#f5f5f5:#e0e0e0\""));
    }

    #[test]
    fn flags_unresolved_endpoint_as_dashed() {
        assert!(node(NodeKind::Unresolved).contains("style=\"filled,dashed\""));
        assert!(node(NodeKind::Cidr).contains("style=\"filled\""));
        assert!(node(NodeKind::PrefixList).contains("fillcolor=\"#eee9fb\""));
    }
}
