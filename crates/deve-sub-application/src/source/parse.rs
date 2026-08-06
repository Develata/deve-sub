//! Content parser: detects the subscription format and dispatches to the
//! appropriate protocol parser.
//!
//! For URI-based formats (URI list, Base64), each line is parsed
//! individually so per-entry failures are recorded. For container formats
//! (Mihomo YAML, sing-box JSON, etc.), the container parser is called once
//! and returns `Vec<Node>`.
//!
//! A maximum node count is enforced to protect against YAML/JSON bombs
//! (SEC-005). See `docs/plan/milestones/M4-sources-and-node-pool.md`.

use base64::Engine;
use deve_sub_domain::{
    ItemParseStatus, Node, ProtocolConfig, ProtocolKind, ReconcileEntry, SourceType,
};
use deve_sub_protocol::ParseError;
use thiserror::Error;

/// Maximum number of nodes accepted from a single source refresh.
///
/// WHY: combined with the HTTP body size limit in the fetcher, this caps
/// the memory impact of a YAML/JSON bomb (SEC-005). A 10 MB response with
/// 10 000 nodes is a generous upper bound for real subscriptions.
const MAX_NODES: usize = 10_000;

/// Errors produced by content parsing.
#[derive(Debug, Error)]
pub enum ParseContentError {
    /// The response body was not valid UTF-8.
    #[error("invalid UTF-8 in response body: {0}")]
    InvalidUtf8(String),

    /// The content could not be parsed as the detected or configured format.
    #[error("parse error: {0}")]
    Parse(String),

    /// The parsed node count exceeded the safety limit.
    #[error("too many nodes: {0} (max {MAX_NODES})")]
    TooManyNodes(usize),
}

/// Parse fetched content into entries ready for reconciliation.
///
/// Format detection follows `source_type`; when `Auto`, the content type
/// header and body heuristics are used.
///
/// # Errors
/// - [`ParseContentError::InvalidUtf8`] — the body is not valid UTF-8.
/// - [`ParseContentError::Parse`] — the content could not be parsed.
/// - [`ParseContentError::TooManyNodes`] — node count exceeded the limit.
pub fn parse_content(
    source_type: SourceType,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<Vec<ReconcileEntry>, ParseContentError> {
    let text =
        std::str::from_utf8(body).map_err(|e| ParseContentError::InvalidUtf8(e.to_string()))?;

    let entries = match source_type {
        SourceType::UriList => parse_uri_list_text(text),
        SourceType::Base64 => parse_base64_text(text)?,
        SourceType::MihomoYaml => {
            parse_container(text, deve_sub_protocol::container::parse_mihomo_yaml)?
        }
        SourceType::SingboxJson => {
            parse_container(text, deve_sub_protocol::container::parse_singbox_json)?
        }
        SourceType::XrayJson => {
            parse_container(text, deve_sub_protocol::container::parse_xray_json)?
        }
        SourceType::V2rayJson => {
            parse_container(text, deve_sub_protocol::container::parse_v2ray_json)?
        }
        SourceType::Shadowrocket => {
            parse_container(text, deve_sub_protocol::container::parse_shadowrocket)?
        }
        SourceType::Auto => auto_detect_and_parse(content_type, text)?,
    };

    let node_count = entries.iter().filter(|e| e.node.is_some()).count();
    if node_count > MAX_NODES {
        return Err(ParseContentError::TooManyNodes(node_count));
    }

    Ok(entries)
}

/// Parse a URI list: one URI per line, skipping blanks and comments.
fn parse_uri_list_text(text: &str) -> Vec<ReconcileEntry> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(parse_single_uri)
        .collect()
}

/// Parse a single URI into a `ReconcileEntry`.
fn parse_single_uri(uri: &str) -> ReconcileEntry {
    match deve_sub_protocol::parse_uri(uri) {
        Ok(node) => {
            let status = entry_status(&node);
            ReconcileEntry {
                raw_uri: uri.to_owned(),
                initial_status: status,
                node: Some(node),
            }
        }
        Err(_) => ReconcileEntry {
            raw_uri: uri.to_owned(),
            initial_status: ItemParseStatus::Failed,
            node: None,
        },
    }
}

/// Decode a Base64 subscription and parse as a URI list.
fn parse_base64_text(text: &str) -> Result<Vec<ReconcileEntry>, ParseContentError> {
    let trimmed = text.trim();
    let decoded = try_decode_base64(trimmed).ok_or_else(|| {
        ParseContentError::Parse("failed to decode base64 subscription".to_owned())
    })?;
    let decoded_str =
        String::from_utf8(decoded).map_err(|e| ParseContentError::InvalidUtf8(e.to_string()))?;
    Ok(parse_uri_list_text(&decoded_str))
}

/// Try standard and URL-safe base64 decoders.
fn try_decode_base64(text: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
    if let Ok(decoded) = STANDARD.decode(text) {
        return Some(decoded);
    }
    URL_SAFE_NO_PAD.decode(text).ok()
}

/// Call a container format parser and wrap each `Node` in a `ReconcileEntry`.
fn parse_container(
    text: &str,
    parser: fn(&str) -> Result<Vec<Node>, ParseError>,
) -> Result<Vec<ReconcileEntry>, ParseContentError> {
    let nodes = parser(text).map_err(|e| ParseContentError::Parse(e.to_string()))?;
    Ok(nodes
        .into_iter()
        .map(|node| {
            let raw = node
                .source
                .raw_uri
                .clone()
                .unwrap_or_else(|| node.display_name.clone());
            let status = entry_status(&node);
            ReconcileEntry {
                raw_uri: raw,
                initial_status: status,
                node: Some(node),
            }
        })
        .collect())
}

/// Determine the parse status from a node's protocol config.
fn entry_status(node: &Node) -> ItemParseStatus {
    match &node.config {
        ProtocolConfig::Unsupported(_) => ItemParseStatus::Unsupported,
        _ => match &node.protocol {
            // WHY: Unknown protocol kind implies the node is not P0-supported,
            // even if the config is not explicitly Unsupported.
            ProtocolKind::Unknown(_) => ItemParseStatus::Unsupported,
            _ => ItemParseStatus::Parsed,
        },
    }
}

/// Auto-detect the format from content-type and body heuristics.
fn auto_detect_and_parse(
    content_type: Option<&str>,
    text: &str,
) -> Result<Vec<ReconcileEntry>, ParseContentError> {
    let ct = content_type.unwrap_or("").to_ascii_lowercase();
    let trimmed = text.trim_start();

    if ct.contains("yaml") || trimmed.contains("proxies:") || trimmed.contains("proxy-groups:") {
        return parse_container(text, deve_sub_protocol::container::parse_mihomo_yaml);
    }
    if ct.contains("json") || trimmed.starts_with('{') || trimmed.starts_with('[') {
        return try_json_formats(text);
    }
    if try_decode_base64(trimmed).is_some() {
        return parse_base64_text(text);
    }
    Ok(parse_uri_list_text(text))
}

/// Try JSON container formats in order: sing-box, Xray, V2Ray.
fn try_json_formats(text: &str) -> Result<Vec<ReconcileEntry>, ParseContentError> {
    if let Ok(entries) = parse_container(text, deve_sub_protocol::container::parse_singbox_json) {
        return Ok(entries);
    }
    if let Ok(entries) = parse_container(text, deve_sub_protocol::container::parse_xray_json) {
        return Ok(entries);
    }
    parse_container(text, deve_sub_protocol::container::parse_v2ray_json)
}
