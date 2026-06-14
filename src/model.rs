//! The domain model — the pivot representation of the pipeline.
//!
//! This module knows nothing about HCL (the input) nor DOT (the output). It is
//! the stable core that `build` produces and `render` consumes.

/// A network-flow graph: nodes (resources + external endpoints), the flows
/// between them, and the visual zones (groups) nodes belong to.
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Visual zones rendered as clusters. A node references one by `Node.group`.
    pub groups: Vec<Group>,
}

/// A graph node. Its `id` is the canonical identity used both for
/// deduplication and as the DOT node identifier.
pub struct Node {
    pub id: String,
    pub label: String,
    /// Origin reference kept as description (e.g. `local.something_cidr`).
    pub tooltip: Option<String>,
    pub kind: NodeKind,
    /// The id of the [`Group`] this node belongs to, if any. Ungrouped nodes
    /// (`None`) render at the top level.
    pub group: Option<String>,
}

/// A visual zone wrapping a set of nodes, rendered as a DOT cluster. Membership
/// is carried by [`Node::group`]; this struct holds the zone's metadata.
pub struct Group {
    /// Stable identity, also the basis of the DOT cluster id.
    pub id: String,
    /// Human-readable cluster label (e.g. `VPC vpc-0cda…`, `External`).
    pub label: String,
    pub kind: GroupKind,
}

/// The semantic category of a [`Group`]. Drives the cluster's palette; carries
/// no colour itself.
pub enum GroupKind {
    /// A VPC the internal resources live in.
    Vpc,
    /// The outside world: CIDR blocks, prefix lists, unresolved references.
    External,
    /// A user-named zone (`local.flowghetti.groups`) peeling specific external
    /// endpoints out of `External` into a labelled cluster.
    Named,
}

pub enum NodeKind {
    /// An internal security group, declared in `var.ressources`.
    Resource { type_: String },
    /// A set of CIDR blocks.
    Cidr,
    /// A set of prefix-list ids.
    PrefixList,
    /// An unresolved reference (e.g. a `data.*` source).
    Unresolved,
}

/// The port(s) a flow targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortSpec {
    Single(i64),
    Range(i64, i64),
    /// Neither `port` nor `port_range` was usable.
    Any,
}

/// A flow, oriented source → destination.
///
/// The edge carries the protocol and port(s) as structured domain data; the
/// human-readable `protocol/port` text is a derived property (see [`Edge::label`]).
/// `render` emits that label and may additionally key styling off `protocol` /
/// `port`; nothing pre-formats the label upstream.
pub struct Edge {
    pub from: String,
    pub to: String,
    pub protocol: String,
    pub port: PortSpec,
}

impl Edge {
    /// The `protocol/port` text shown on the edge — e.g. `tcp/443`,
    /// `tcp/10000-10100`, `all` (protocol `all`), or a bare protocol when no
    /// usable port was found. Pure function of `protocol` and `port`.
    pub fn label(&self) -> String {
        if self.protocol == "all" {
            return "all".to_string();
        }
        match &self.port {
            PortSpec::Single(p) => format!("{}/{p}", self.protocol),
            PortSpec::Range(from, to) => format!("{}/{from}-{to}", self.protocol),
            PortSpec::Any => self.protocol.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(protocol: &str, port: PortSpec) -> Edge {
        Edge {
            from: "a".to_string(),
            to: "b".to_string(),
            protocol: protocol.to_string(),
            port,
        }
    }

    #[test]
    fn label_formats_protocol_and_port() {
        assert_eq!(edge("tcp", PortSpec::Single(443)).label(), "tcp/443");
        assert_eq!(
            edge("udp", PortSpec::Range(10000, 10100)).label(),
            "udp/10000-10100"
        );
        assert_eq!(edge("all", PortSpec::Single(0)).label(), "all");
        assert_eq!(edge("tcp", PortSpec::Any).label(), "tcp");
    }
}
