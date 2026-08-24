//! Shadowrocket share list parser.
//!
//! Shadowrocket exports nodes as a list of share URIs, one per line —
//! the same format as [`super::uri_list`]. Exports are commonly base64
//! encoded; this parser decodes base64 first and falls back to plain text
//! so both forms round-trip through the emitter (which always base64
//! encodes). See R3-16.

use base64::Engine;

use deve_sub_domain::Node;

use crate::error::ParseError;

/// Parse a Shadowrocket share list into a list of [`Node`] values.
///
/// Shadowrocket share lists are base64-encoded URI lists. This function
/// decodes the base64 first; if decoding fails or yields non-UTF-8, it
/// falls back to treating the input as a plain URI list (the format
/// `uri_list` accepts).
///
/// # Errors
/// See [`super::parse_uri_list`]; always returns `Ok`.
pub fn parse_shadowrocket(text: &str) -> Result<Vec<Node>, ParseError> {
    let trimmed = text.trim();
    // WHY: Shadowrocket exports are base64-encoded, but the emitter in
    // container/mod.rs base64-encodes its output, so re-parsing emitter
    // output requires a base64 decode step. Try base64 first; fall back to
    // plain text so hand-written plain URI lists still parse. See R3-16.
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(trimmed.as_bytes())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|s| {
            // WHY: a plain URI list contains `://` on every non-empty line;
            // a base64-decoded string of a plain-URI-list-without-`://`
            // prefix is unlikely to contain `://`. Guard against the edge
            // case where the plain input itself happens to be valid base64
            // (rare for multi-line URI lists but possible for short ones).
            s.contains("://") || s.lines().all(|l| l.is_empty())
        });
    let body = decoded.as_deref().unwrap_or(text);
    super::uri_list::parse_uri_list(body)
}
