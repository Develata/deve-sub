//! Node-level chain graph and cycle detection.
//!
//! Reuses the DFS three-color marking algorithm from M5's
//! [`crate::template::ChainGraph`], applied at the node level. Each node's
//! `chain` field lists the nodes its traffic traverses; the directed graph
//! has edges from each node to each node in its chain. A cycle means a node
//! indirectly depends on itself.
//!
//! See M7 plan §"Node chain proxy" and NODE-018.

use std::collections::HashMap;
use std::fmt;

use deve_sub_kernel::NodeId;
use thiserror::Error;

/// A cycle detected in the node chain graph, with the full node path.
///
/// The `nodes` vector lists the cycle in order, with the first node repeated
/// at the end. For example, a cycle A → B → A is stored as `[A, B, A]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeCyclePath {
    /// The node IDs forming the cycle, in order, with the first repeated
    /// at the end.
    pub nodes: Vec<NodeId>,
}

impl fmt::Display for NodeCyclePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for id in &self.nodes {
            if !first {
                f.write_str(" -> ")?;
            }
            first = false;
            write!(f, "{id}")?;
        }
        Ok(())
    }
}

impl std::error::Error for NodeCyclePath {}

/// Validation errors for a node chain operation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NodeChainError {
    /// The chain list is empty.
    #[error("chain must not be empty")]
    Empty,
    /// The node's own ID appears in its chain.
    #[error("chain must not contain the node itself")]
    SelfReference,
    /// The chain contains a duplicate entry.
    #[error("chain contains duplicate node: {0}")]
    Duplicate(NodeId),
    /// One or more referenced nodes do not exist in the pool.
    #[error("chain references non-existent nodes: {0:?}")]
    NodeNotFound(Vec<NodeId>),
    /// A cycle was detected. The path is included for diagnostics.
    #[error("chain cycle detected: {0}")]
    Cycle(#[from] NodeCyclePath),
}

/// The directed node-level chain graph.
///
/// Each edge N → M means "N's chain includes M" (N depends on M). Built from
/// a snapshot of all nodes' chains. Cycle detection uses DFS three-color
/// marking, identical in structure to M5's `ChainGraph::detect_cycle`.
pub struct NodeChainGraph {
    adjacency: HashMap<NodeId, Vec<NodeId>>,
}

impl NodeChainGraph {
    /// Build the graph from a slice of `(node_id, chain)` pairs. Nodes with
    /// `chain = None` produce no outgoing edges but are still vertices.
    pub fn from_chains(chains: &[(NodeId, Option<Vec<NodeId>>)]) -> Self {
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for (node_id, chain) in chains {
            let entry = adjacency.entry(*node_id).or_default();
            if let Some(targets) = chain {
                entry.extend_from_slice(targets);
            }
        }
        Self { adjacency }
    }

    /// Detect cycles using DFS three-color marking.
    ///
    /// Returns the first cycle found, or `None` if the graph is acyclic.
    pub fn detect_cycle(&self) -> Option<NodeCyclePath> {
        let mut color: HashMap<&NodeId, Color> = HashMap::new();
        let mut path: Vec<&NodeId> = Vec::new();

        let mut roots: Vec<&NodeId> = self.adjacency.keys().collect();
        roots.sort();

        for v in roots {
            if !color.contains_key(v)
                && let Some(cycle) = self.dfs_visit(v, &mut color, &mut path)
            {
                return Some(cycle);
            }
        }
        None
    }

    fn dfs_visit<'a>(
        &'a self,
        v: &'a NodeId,
        color: &mut HashMap<&'a NodeId, Color>,
        path: &mut Vec<&'a NodeId>,
    ) -> Option<NodeCyclePath> {
        color.insert(v, Color::Gray);
        path.push(v);

        if let Some(neighbors) = self.adjacency.get(v) {
            let mut sorted: Vec<&NodeId> = neighbors.iter().collect();
            sorted.sort();

            for neighbor in sorted {
                let neighbor_color = color.get(neighbor).copied().unwrap_or(Color::White);
                match neighbor_color {
                    Color::White => {
                        if let Some(cycle) = self.dfs_visit(neighbor, color, path) {
                            return Some(cycle);
                        }
                    }
                    Color::Gray => {
                        let cycle_start = path.iter().position(|x| **x == *neighbor).unwrap_or(0);
                        let mut nodes: Vec<NodeId> =
                            path[cycle_start..].iter().map(|x| **x).collect();
                        nodes.push(*neighbor);
                        return Some(NodeCyclePath { nodes });
                    }
                    Color::Black => {}
                }
            }
        }

        path.pop();
        color.insert(v, Color::Black);
        None
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Color {
    White,
    Gray,
    Black,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nid(s: &str) -> NodeId {
        NodeId::parse(s).expect("valid ULID")
    }

    #[test]
    fn empty_graph_has_no_cycle() {
        let graph = NodeChainGraph::from_chains(&[]);
        assert!(graph.detect_cycle().is_none());
    }

    #[test]
    fn single_node_no_chain_no_cycle() {
        let a = nid("01KZAAAAAAAAAAAAAAAAAAAAAA");
        let graph = NodeChainGraph::from_chains(&[(a, None)]);
        assert!(graph.detect_cycle().is_none());
    }

    #[test]
    fn linear_chain_no_cycle() {
        let a = nid("01KZAAAAAAAAAAAAAAAAAAAAAA");
        let b = nid("01KZBBBBBBBBBBBBBBBBBBBBBB");
        let c = nid("01KZCCCCCCCCCCCCCCCCCCCCCC");
        let graph = NodeChainGraph::from_chains(&[(a, Some(vec![b])), (b, Some(vec![c]))]);
        assert!(graph.detect_cycle().is_none());
    }

    #[test]
    fn self_reference_is_cycle() {
        let a = nid("01KZAAAAAAAAAAAAAAAAAAAAAA");
        let graph = NodeChainGraph::from_chains(&[(a, Some(vec![a]))]);
        let cycle = graph.detect_cycle().expect("cycle");
        assert_eq!(cycle.nodes.first(), cycle.nodes.last());
        assert!(cycle.nodes.len() >= 2);
    }

    #[test]
    fn two_node_cycle_detected() {
        let a = nid("01KZAAAAAAAAAAAAAAAAAAAAAA");
        let b = nid("01KZBBBBBBBBBBBBBBBBBBBBBB");
        let graph = NodeChainGraph::from_chains(&[(a, Some(vec![b])), (b, Some(vec![a]))]);
        let cycle = graph.detect_cycle().expect("cycle");
        assert!(cycle.nodes.len() >= 3);
        assert_eq!(cycle.nodes.first(), cycle.nodes.last());
    }

    #[test]
    fn three_node_cycle_detected() {
        let a = nid("01KZAAAAAAAAAAAAAAAAAAAAAA");
        let b = nid("01KZBBBBBBBBBBBBBBBBBBBBBB");
        let c = nid("01KZCCCCCCCCCCCCCCCCCCCCCC");
        let graph = NodeChainGraph::from_chains(&[
            (a, Some(vec![b])),
            (b, Some(vec![c])),
            (c, Some(vec![a])),
        ]);
        let cycle = graph.detect_cycle().expect("cycle");
        assert!(cycle.nodes.len() >= 4);
        assert_eq!(cycle.nodes.first(), cycle.nodes.last());
    }

    #[test]
    fn cycle_path_display() {
        let path = NodeCyclePath {
            nodes: vec![
                nid("01KZAAAAAAAAAAAAAAAAAAAAAA"),
                nid("01KZBBBBBBBBBBBBBBBBBBBBBB"),
                nid("01KZAAAAAAAAAAAAAAAAAAAAAA"),
            ],
        };
        let s = path.to_string();
        assert!(s.contains("->"));
        assert_eq!(s.matches("->").count(), 2);
    }

    #[test]
    fn validate_structure_empty_rejected() {
        let chain = crate::NodeChain { nodes: vec![] };
        let self_id = nid("01KZAAAAAAAAAAAAAAAAAAAAAA");
        assert_eq!(
            chain.validate_structure(self_id),
            Err(NodeChainError::Empty)
        );
    }

    #[test]
    fn validate_structure_self_reference_rejected() {
        let self_id = nid("01KZAAAAAAAAAAAAAAAAAAAAAA");
        let other = nid("01KZBBBBBBBBBBBBBBBBBBBBBB");
        let chain = crate::NodeChain {
            nodes: vec![other, self_id],
        };
        assert_eq!(
            chain.validate_structure(self_id),
            Err(NodeChainError::SelfReference)
        );
    }

    #[test]
    fn validate_structure_duplicate_rejected() {
        let self_id = nid("01KZAAAAAAAAAAAAAAAAAAAAAA");
        let other = nid("01KZBBBBBBBBBBBBBBBBBBBBBB");
        let chain = crate::NodeChain {
            nodes: vec![other, other],
        };
        assert_eq!(
            chain.validate_structure(self_id),
            Err(NodeChainError::Duplicate(other))
        );
    }

    #[test]
    fn validate_structure_valid_passes() {
        let self_id = nid("01KZAAAAAAAAAAAAAAAAAAAAAA");
        let a = nid("01KZBBBBBBBBBBBBBBBBBBBBBB");
        let b = nid("01KZCCCCCCCCCCCCCCCCCCCCCC");
        let chain = crate::NodeChain { nodes: vec![a, b] };
        assert!(chain.validate_structure(self_id).is_ok());
    }
}
