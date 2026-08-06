//! Base64 subscription decoder.
//!
//! A Base64 subscription is the entire response body Base64-encoded. When
//! decoded, it contains one share URI per line (see [`super::uri_list`]).
//! Both padded and unpadded, standard and URL-safe Base64 are accepted
//! (PARSE-010).

use deve_sub_domain::Node;

use crate::error::ParseError;

/// Parse a Base64-encoded subscription body into a list of [`Node`] values.
///
/// The input is Base64-decoded (trying standard and URL-safe, padded and
/// unpadded variants), then parsed as a URI list. Unknown-scheme URIs are
/// preserved as `UnsupportedNode`; malformed lines are skipped.
///
/// # Errors
/// Returns [`ParseError::InvalidBase64`] if the input is not valid Base64.
/// Returns [`ParseError::InvalidField`] if the decoded bytes are not valid
/// UTF-8.
pub fn parse_base64_subscription(text: &str) -> Result<Vec<Node>, ParseError> {
    let decoded = crate::uri::decode_base64_flexible(text.trim())?;
    let decoded_str = String::from_utf8(decoded).map_err(|e| ParseError::InvalidField {
        field: "base64 body",
        value: e.to_string(),
    })?;
    super::uri_list::parse_uri_list(&decoded_str)
}
