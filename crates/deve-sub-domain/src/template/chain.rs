//! Chain proxy directed graph and cycle detection.
//!
//! A `ChainGraph` is built from a template's proxy groups. It captures two
//! kinds of directed edges:
//!
//! - **Relay sequence edges**: for `relay`-type groups, consecutive members
//!   form chain edges `m[i] → m[i+1]` representing traffic flow order. These
//!   cover all four vertex combinations: node→node, node→group, group→node,
//!   group→group (spec §832-839).
//!
//! - **Group dependency edges**: any group (regardless of type) that contains
//!   a `GroupMember::Group { name }` reference creates an edge
//!   `group_name → referenced_group_name`. This captures the dependency
//!   relationship: the referencing group depends on the referenced group's
//!   resolution.
//!
//! Cycles in this graph indicate infinite loops in chain proxy resolution and
//! are rejected at save time via DFS three-color marking (GEN-012).

use std::collections::HashMap;
use std::fmt;

use deve_sub_kernel::NodeId;

use super::spec::{GroupMember, GroupType, ProxyGroup};

/// A vertex in the chain dependency graph — either a pool node or a proxy
/// group identified by name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChainVertex {
    /// A concrete node from the pool.
    Node(NodeId),
    /// A proxy group referenced by name.
    Group(String),
}

impl fmt::Display for ChainVertex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Node(id) => write!(f, "node:{id}"),
            Self::Group(name) => write!(f, "group:{name}"),
        }
    }
}

/// A directed edge in the chain graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainEdge {
    /// The source vertex.
    pub from: ChainVertex,
    /// The destination vertex.
    pub to: ChainVertex,
}

/// A cycle detected in the chain graph, with the full vertex path.
///
/// The `vertices` vector lists the cycle in order, with the first vertex
/// repeated at the end to close the cycle. For example, a cycle A → B → A
/// is stored as `[A, B, A]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CyclePath {
    /// The vertices forming the cycle, in order, with the first vertex
    /// repeated at the end.
    pub vertices: Vec<ChainVertex>,
}

impl fmt::Display for CyclePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for v in &self.vertices {
            if !first {
                f.write_str(" -> ")?;
            }
            first = false;
            write!(f, "{v}")?;
        }
        Ok(())
    }
}

/// The directed chain graph built from a template's proxy groups.
///
/// See the module-level documentation for edge semantics.
#[derive(Debug, Clone, Default)]
pub struct ChainGraph {
    adjacency: HashMap<ChainVertex, Vec<ChainVertex>>,
}

impl ChainGraph {
    /// Build the chain graph from a slice of proxy groups.
    pub fn from_groups(groups: &[ProxyGroup]) -> Self {
        let mut adjacency: HashMap<ChainVertex, Vec<ChainVertex>> = HashMap::new();

        for group in groups {
            let group_vertex = ChainVertex::Group(group.name.clone());

            for member in &group.members {
                if let GroupMember::Group { name } = member {
                    adjacency
                        .entry(group_vertex.clone())
                        .or_default()
                        .push(ChainVertex::Group(name.clone()));
                }
            }

            if group.group_type == GroupType::Relay {
                for window in group.members.windows(2) {
                    let from = member_to_vertex(&window[0]);
                    let to = member_to_vertex(&window[1]);
                    adjacency.entry(from).or_default().push(to);
                }
            }
        }

        Self { adjacency }
    }

    /// Detect cycles using DFS three-color marking.
    ///
    /// Returns the first cycle found, or `None` if the graph is acyclic.
    pub fn detect_cycle(&self) -> Option<CyclePath> {
        crate::graph::first_cycle(&self.adjacency, compare_vertices).map(|vertices| CyclePath {
            vertices: vertices.into_iter().cloned().collect(),
        })
    }

    /// List all edges in the graph.
    pub fn edges(&self) -> Vec<ChainEdge> {
        self.adjacency
            .iter()
            .flat_map(|(from, neighbors)| {
                neighbors.iter().map(|to| ChainEdge {
                    from: from.clone(),
                    to: to.clone(),
                })
            })
            .collect()
    }
}

// Preserve the former display-string order ("group:" before "node:")
// without allocating a display string for each comparison.
fn compare_vertices(a: &ChainVertex, b: &ChainVertex) -> std::cmp::Ordering {
    match (a, b) {
        (ChainVertex::Group(a), ChainVertex::Group(b)) => a.cmp(b),
        (ChainVertex::Node(a), ChainVertex::Node(b)) => a.cmp(b),
        (ChainVertex::Group(_), ChainVertex::Node(_)) => std::cmp::Ordering::Less,
        (ChainVertex::Node(_), ChainVertex::Group(_)) => std::cmp::Ordering::Greater,
    }
}

fn member_to_vertex(m: &GroupMember) -> ChainVertex {
    match m {
        GroupMember::Node { id } => ChainVertex::Node(*id),
        GroupMember::Group { name } => ChainVertex::Group(name.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_group_references_use_bounded_call_stack() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let mut groups: Vec<_> = (0..4_000)
                    .map(|i| ProxyGroup {
                        name: format!("g{i:04}"),
                        group_type: GroupType::Select,
                        members: if i < 3_999 {
                            vec![GroupMember::Group {
                                name: format!("g{:04}", i + 1),
                            }]
                        } else {
                            Vec::new()
                        },
                        filter: None,
                        sort_order: None,
                    })
                    .collect();
                assert!(ChainGraph::from_groups(&groups).detect_cycle().is_none());
                groups[3_999].members.push(GroupMember::Group {
                    name: "g0000".into(),
                });
                let cycle = ChainGraph::from_groups(&groups)
                    .detect_cycle()
                    .expect("cycle");
                let mut expected: Vec<_> = groups
                    .iter()
                    .map(|g| ChainVertex::Group(g.name.clone()))
                    .collect();
                expected.push(ChainVertex::Group("g0000".into()));
                assert_eq!(cycle.vertices, expected);
            })
            .expect("thread")
            .join()
            .expect("bounded stack traversal");
    }

    fn node(id: &str) -> GroupMember {
        GroupMember::Node {
            id: NodeId::parse(id).expect("valid ULID"),
        }
    }

    fn group(name: &str) -> GroupMember {
        GroupMember::Group {
            name: name.to_owned(),
        }
    }

    fn make_relay(name: &str, members: Vec<GroupMember>) -> ProxyGroup {
        ProxyGroup {
            name: name.to_owned(),
            group_type: GroupType::Relay,
            members,
            filter: None,
            sort_order: None,
        }
    }

    fn make_select(name: &str, members: Vec<GroupMember>) -> ProxyGroup {
        ProxyGroup {
            name: name.to_owned(),
            group_type: GroupType::Select,
            members,
            filter: None,
            sort_order: None,
        }
    }

    #[test]
    fn empty_groups_produce_empty_graph() {
        let graph = ChainGraph::from_groups(&[]);
        assert!(graph.edges().is_empty());
        assert!(graph.detect_cycle().is_none());
    }

    #[test]
    fn relay_node_chain_no_cycle() {
        let relay = make_relay(
            "chain",
            vec![
                node("01KZAAAAAAAAAAAAAAAAAAAAAA"),
                node("01KZBBBBBBBBBBBBBBBBBBBBBB"),
                node("01KZCCCCCCCCCCCCCCCCCCCCCC"),
            ],
        );
        let graph = ChainGraph::from_groups(&[relay]);
        assert_eq!(graph.edges().len(), 2);
        assert!(graph.detect_cycle().is_none());
    }

    #[test]
    fn relay_with_group_ref_no_cycle() {
        let inner = make_select("inner", vec![node("01KZAAAAAAAAAAAAAAAAAAAAAA")]);
        let relay = make_relay(
            "outer",
            vec![node("01KZBBBBBBBBBBBBBBBBBBBBBB"), group("inner")],
        );
        let graph = ChainGraph::from_groups(&[inner, relay]);
        assert!(graph.detect_cycle().is_none());
    }

    #[test]
    fn two_relays_no_cycle() {
        let relay1 = make_relay("r1", vec![node("01KZAAAAAAAAAAAAAAAAAAAAAA"), group("r2")]);
        let relay2 = make_relay(
            "r2",
            vec![
                node("01KZBBBBBBBBBBBBBBBBBBBBBB"),
                node("01KZCCCCCCCCCCCCCCCCCCCCCC"),
            ],
        );
        let graph = ChainGraph::from_groups(&[relay1, relay2]);
        assert!(graph.detect_cycle().is_none());
    }

    #[test]
    fn self_referencing_group_cycle() {
        let relay = make_relay(
            "loop",
            vec![node("01KZAAAAAAAAAAAAAAAAAAAAAA"), group("loop")],
        );
        let graph = ChainGraph::from_groups(&[relay]);
        let cycle = graph.detect_cycle().expect("cycle");
        assert!(cycle.vertices.len() >= 2);
        assert_eq!(cycle.vertices.first(), cycle.vertices.last());
    }

    #[test]
    fn mutual_group_reference_cycle() {
        let g1 = make_select("g1", vec![group("g2")]);
        let g2 = make_select("g2", vec![group("g1")]);
        let graph = ChainGraph::from_groups(&[g1, g2]);
        let cycle = graph.detect_cycle().expect("cycle");
        assert!(cycle.vertices.len() >= 3);
        assert_eq!(cycle.vertices.first(), cycle.vertices.last());
    }

    #[test]
    fn relay_cross_reference_cycle() {
        let r1 = make_relay("r1", vec![node("01KZAAAAAAAAAAAAAAAAAAAAAA"), group("r2")]);
        let r2 = make_relay("r2", vec![group("r1"), node("01KZBBBBBBBBBBBBBBBBBBBBBB")]);
        let graph = ChainGraph::from_groups(&[r1, r2]);
        let cycle = graph.detect_cycle().expect("cycle");
        assert!(cycle.vertices.len() >= 3);
        assert_eq!(cycle.vertices.first(), cycle.vertices.last());
    }

    #[test]
    fn cycle_path_display() {
        let path = CyclePath {
            vertices: vec![
                ChainVertex::Group("a".to_owned()),
                ChainVertex::Group("b".to_owned()),
                ChainVertex::Group("a".to_owned()),
            ],
        };
        assert_eq!(path.to_string(), "group:a -> group:b -> group:a");
    }

    #[test]
    fn edges_include_both_relay_and_dependency() {
        let inner = make_select("inner", vec![node("01KZAAAAAAAAAAAAAAAAAAAAAA")]);
        let relay = make_relay(
            "outer",
            vec![node("01KZBBBBBBBBBBBBBBBBBBBBBB"), group("inner")],
        );
        let graph = ChainGraph::from_groups(&[inner, relay]);
        let edges = graph.edges();

        assert!(edges.iter().any(|e| {
            e.from == ChainVertex::Node(NodeId::parse("01KZBBBBBBBBBBBBBBBBBBBBBB").expect("ulid"))
                && e.to == ChainVertex::Group("inner".to_owned())
        }));
        assert!(edges.iter().any(|e| {
            e.from == ChainVertex::Group("outer".to_owned())
                && e.to == ChainVertex::Group("inner".to_owned())
        }));
    }
}
