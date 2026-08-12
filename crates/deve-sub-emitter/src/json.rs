//! JSON output profile emitter.
//!
//! Serializes the canonical [`deve_sub_domain::Node`] array as a JSON
//! document via `serde_json::to_string_pretty`. This is the full-fidelity
//! profile: every protocol, transport, and field is preserved verbatim,
//! with no target-client filtering (see M9 Slice 5, ADR-0003).
//!
//! The schema is the `Node` type's serde representation. Round-trip is
//! `emit_json(nodes)` → `serde_json::from_str::<Vec<Node>>` → semantic
//! equality.

use deve_sub_domain::Node;

use crate::error::EmitError;

/// Serialize a slice of canonical nodes as a pretty-printed JSON array.
///
/// # Errors
/// Returns [`EmitError::Encode`] if serde serialization fails (in practice
/// it cannot for `Node`, which derives `Serialize`).
pub fn emit_json(nodes: &[Node]) -> Result<String, EmitError> {
    serde_json::to_string_pretty(nodes).map_err(|e| EmitError::Encode(e.to_string()))
}
