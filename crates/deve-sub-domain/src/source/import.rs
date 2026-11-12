//! Manual import outcomes and usable-node accounting.

use crate::Node;
use deve_sub_kernel::NodeId;

/// Result of a manual node import (NODE-001 / NODE-002).
///
/// Each input is inserted, duplicated, retained as unsupported, or rejected.
/// Unsupported entries are counted separately from usable nodes. The counts
/// mirror the per-row `ItemParseStatus` used by source refresh so the UI
/// and CLI can render a single import summary.
#[derive(Debug, Clone, Default)]
pub struct ImportResult {
    /// Nodes newly inserted into the pool.
    pub new_nodes: u64,
    /// Nodes already present (same protocol + host + port, active and
    /// non-missing). Not re-inserted; credentials are NOT overwritten
    /// (NODE-003: do not drop nodes with different credentials).
    pub duplicate_nodes: u64,
    /// Input lines that could not be parsed at all.
    pub failed: u64,
    /// Raw entries retained for inspection but not usable for generation.
    pub unsupported: u64,
    /// Per-line outcomes for diagnostics. Length equals the input line
    /// count; order preserved. Failed lines carry the raw URI; successful
    /// lines carry the resulting `NodeId`.
    pub outcomes: Vec<ImportOutcome>,
}

/// Per-line outcome of a manual import.
#[derive(Debug, Clone)]
pub enum ImportOutcome {
    /// Raw entry retained without claiming protocol support.
    Unsupported { id: NodeId, reason: String },
    /// A new node was inserted; carries its `NodeId`.
    Inserted(NodeId),
    /// The node was a duplicate of an existing active pool entry; carries
    /// the existing node's `NodeId`.
    Duplicate(NodeId),
    /// The line could not be parsed; carries the raw input text.
    Failed(String),
}

impl ImportResult {
    /// Record one persisted entry without counting unsupported raw data as a usable import.
    pub fn record_node(&mut self, node: &Node, id: NodeId, duplicate: bool) {
        if let crate::ProtocolConfig::Unsupported(raw) = &node.config {
            self.unsupported += 1;
            self.outcomes.push(ImportOutcome::Unsupported {
                id,
                reason: raw.reason.clone(),
            });
        } else if duplicate {
            self.duplicate_nodes += 1;
            self.outcomes.push(ImportOutcome::Duplicate(id));
        } else {
            self.new_nodes += 1;
            self.outcomes.push(ImportOutcome::Inserted(id));
        }
    }
}
