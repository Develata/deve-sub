//! Shared depth-first traversal for node and template chain validation.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;

/// Return the first closed cycle in the caller's stable vertex order.
///
/// Gray vertices are exactly the active DFS frames; a gray edge closes a
/// cycle, while exhausting a frame marks it black. Explicit frames keep Rust
/// call-stack use constant even when individually short chains form a deep
/// graph. Traversal is O(V + E), plus root/neighbor sorting for deterministic
/// diagnostics; auxiliary heap memory is O(V + E).
pub(crate) fn first_cycle<V: Eq + Hash>(
    adjacency: &HashMap<V, Vec<V>>,
    compare: impl Fn(&V, &V) -> Ordering,
) -> Option<Vec<&V>> {
    let mut roots: Vec<_> = adjacency.keys().collect();
    roots.sort_unstable_by(|a, b| compare(a, b));
    let mut color: HashMap<&V, Color> = HashMap::new();
    let frame = |node| {
        let mut neighbors: Vec<_> = adjacency.get(node).into_iter().flatten().collect();
        neighbors.sort_unstable_by(|a, b| compare(a, b));
        Frame {
            node,
            neighbors: neighbors.into_iter(),
        }
    };

    for root in roots {
        if color.contains_key(root) {
            continue;
        }
        color.insert(root, Color::Gray);
        let mut stack = vec![frame(root)];
        while let Some(current) = stack.last_mut() {
            let Some(neighbor) = current.neighbors.next() else {
                color.insert(current.node, Color::Black);
                stack.pop();
                continue;
            };
            match color.get(neighbor) {
                None => {
                    color.insert(neighbor, Color::Gray);
                    stack.push(frame(neighbor));
                }
                Some(Color::Gray) => {
                    let mut cycle: Vec<_> = stack
                        .iter()
                        .skip_while(|entry| entry.node != neighbor)
                        .map(|entry| entry.node)
                        .collect();
                    cycle.push(neighbor);
                    return Some(cycle);
                }
                Some(Color::Black) => {}
            }
        }
    }
    None
}

struct Frame<'a, V> {
    node: &'a V,
    neighbors: std::vec::IntoIter<&'a V>,
}

enum Color {
    Gray,
    Black,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_three_vertex_graphs_preserve_the_recursive_cycle_path() {
        // The former recursive algorithm is a bounded oracle here. Exhaust
        // self-loops, shared descendants, disconnected components and cycles,
        // including cases with more than one possible diagnostic path.
        for mask in 0_u16..512 {
            let adjacency: HashMap<u8, Vec<u8>> = (0..3)
                .rev()
                .map(|from| {
                    let neighbors = (0..3)
                        .rev()
                        .filter(|to| mask & (1 << (from * 3 + to)) != 0)
                        .collect();
                    (from, neighbors)
                })
                .collect();
            let actual = first_cycle(&adjacency, u8::cmp)
                .map(|cycle| cycle.into_iter().copied().collect::<Vec<_>>());
            let mut colors = [0; 3];
            let expected = (0..3).find_map(|root| {
                (colors[root as usize] == 0)
                    .then(|| recursive_cycle(&adjacency, root, &mut colors, &mut Vec::new()))
                    .flatten()
            });
            assert_eq!(actual, expected, "edge mask {mask}");
        }
    }

    fn recursive_cycle(
        adjacency: &HashMap<u8, Vec<u8>>,
        node: u8,
        colors: &mut [u8; 3],
        path: &mut Vec<u8>,
    ) -> Option<Vec<u8>> {
        colors[node as usize] = 1;
        path.push(node);
        let mut neighbors = adjacency[&node].clone();
        neighbors.sort_unstable();
        for neighbor in neighbors {
            match colors[neighbor as usize] {
                0 => {
                    if let Some(cycle) = recursive_cycle(adjacency, neighbor, colors, path) {
                        return Some(cycle);
                    }
                }
                1 => {
                    let start = path
                        .iter()
                        .position(|id| *id == neighbor)
                        .expect("gray path");
                    let mut cycle = path[start..].to_vec();
                    cycle.push(neighbor);
                    return Some(cycle);
                }
                _ => {}
            }
        }
        path.pop();
        colors[node as usize] = 2;
        None
    }
}
