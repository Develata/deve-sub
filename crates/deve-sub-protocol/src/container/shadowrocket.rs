//! Shadowrocket share list parser.
//!
//! Shadowrocket exports nodes as a list of share URIs, one per line —
//! the same format as [`super::uri_list`]. This parser delegates directly
//! to [`parse_uri_list`].

use deve_sub_domain::Node;

use crate::error::ParseError;

/// Parse a Shadowrocket share list into a list of [`Node`] values.
///
/// Shadowrocket share lists use the same one-URI-per-line format as a
/// plain URI list. This function delegates to [`super::parse_uri_list`].
///
/// # Errors
/// See [`super::parse_uri_list`]; always returns `Ok`.
pub fn parse_shadowrocket(text: &str) -> Result<Vec<Node>, ParseError> {
    super::uri_list::parse_uri_list(text)
}
