//! URI list parser: one share URI per line.
//!
//! Lines starting with `#` are comments. Empty lines are skipped. Each
//! remaining line is parsed via [`crate::parse_uri`]. Unknown-scheme URIs
//! are preserved as `UnsupportedNode` (constraint #7); other parse errors
//! (missing fields, malformed URI) cause the line to be skipped.

use deve_sub_domain::{Node, ProtocolKind};

use crate::error::ParseError;

use super::unsupported_entry;

/// Parse a text block containing one share URI per line.
///
/// Comment lines (`#` prefix) and empty lines are skipped. Unknown-scheme
/// URIs are preserved as `UnsupportedNode`. Lines with other parse errors
/// (missing required fields, malformed URI) are silently skipped —
/// subscription providers often include non-URI content.
///
/// # Errors
/// This function always returns `Ok`. An empty or all-comment input yields
/// an empty `Vec`.
pub fn parse_uri_list(text: &str) -> Result<Vec<Node>, ParseError> {
    let mut nodes = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match crate::parse_uri(trimmed) {
            Ok(node) => nodes.push(node),
            Err(ParseError::UnknownScheme(scheme)) => {
                let raw = serde_json::Value::String(trimmed.to_owned());
                nodes.push(unsupported_entry(
                    &raw,
                    "uri",
                    ProtocolKind::Unknown(scheme),
                    "unknown protocol scheme".to_owned(),
                ));
            }
            Err(_) => { /* skip malformed lines */ }
        }
    }
    Ok(nodes)
}
