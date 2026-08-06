//! URI list emission: one share URI per line (PARSE-016).
//!
//! Emits a list of [`Node`] values as a text block with one share URI per
//! line, using LF (`\n`) line endings. Unsupported nodes (no URI emitter)
//! are silently skipped — they cannot be represented as share URIs.

use deve_sub_domain::Node;

use crate::error::EmitError;

/// Emit a list of [`Node`] values as one share URI per line.
///
/// Each node is emitted via [`crate::emit_uri`]. Nodes whose protocol has
/// no URI emitter (e.g. `Unsupported`) are silently skipped. The output
/// uses LF line endings, no trailing newline.
///
/// # Errors
/// Returns [`EmitError`] if any supported node fails to emit.
pub fn emit_uri_list(nodes: &[Node]) -> Result<String, EmitError> {
    let mut lines = Vec::with_capacity(nodes.len());
    for node in nodes {
        match crate::emit_uri(node) {
            Ok(uri) => lines.push(uri),
            Err(EmitError::NoEmitter(_)) => { /* skip unsupported */ }
            Err(e) => return Err(e),
        }
    }
    Ok(lines.join("\n"))
}
