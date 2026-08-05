//! URI parsing dispatcher and shared helpers.
//!
//! The top-level [`parse_uri`] function dispatches on the URI scheme to the
//! appropriate protocol parser. Shared helpers for host parsing, query
//! parameter extraction, and boolean conversion live here to avoid
//! duplication across protocol parsers.

use deve_sub_domain::{Host, Node};
use deve_sub_kernel::NodeId;

use crate::error::ParseError;

/// Parse a share URI into a canonical [`Node`].
///
/// Dispatches on the URI scheme (`vless://`, `hysteria2://`, `hy2://`,
/// `tuic://`, `naive+https://`, `ss://`, `vmess://`, `trojan://`).
///
/// The returned `Node` has protocol-specific fields populated from the URI.
/// Metadata fields (`source`, `tags`, `region`) are set to defaults; the
/// caller should override them as needed. The `source.raw_uri` field is set
/// to the original URI string.
///
/// # Errors
/// Returns [`ParseError`] if the URI is malformed, the scheme is unknown,
/// or a required field is missing.
pub fn parse_uri(uri: &str) -> Result<Node, ParseError> {
    let parsed = url::Url::parse(uri).map_err(|e| ParseError::InvalidUri(e.to_string()))?;

    match parsed.scheme() {
        "vless" => crate::vless_reality::parse(&parsed, uri),
        other => Err(ParseError::UnknownScheme(other.to_owned())),
    }
}

/// Convert a host string from the URL parser to a domain [`Host`].
pub(crate) fn parse_host(host_str: &str) -> Result<Host, ParseError> {
    if host_str.is_empty() {
        return Err(ParseError::InvalidHost("empty host".to_owned()));
    }
    if let Ok(ipv4) = host_str.parse::<std::net::Ipv4Addr>() {
        return Ok(Host::Ipv4(ipv4));
    }
    if let Ok(ipv6) = host_str.parse::<std::net::Ipv6Addr>() {
        return Ok(Host::Ipv6(ipv6));
    }
    Ok(Host::Domain(deve_sub_domain::DomainName::new(
        host_str.to_owned(),
    )))
}

/// Parse a query parameter value as a boolean.
///
/// Accepts `true`/`false` (case-sensitive) and `1`/`0`.
pub(crate) fn parse_bool(value: &str) -> Result<bool, ParseError> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(ParseError::InvalidField {
            field: "boolean",
            value: value.to_owned(),
        }),
    }
}

/// Create a `Node` with default metadata fields.
///
/// Protocol-specific fields are filled by the caller. The `source.raw_uri`
/// is set to the original URI string for provenance.
pub(crate) fn node_shell(raw_uri: &str) -> Node {
    Node {
        id: NodeId::new(),
        display_name: String::new(),
        protocol: deve_sub_domain::ProtocolKind::Unknown(String::new()),
        config: deve_sub_domain::ProtocolConfig::Unsupported(deve_sub_domain::UnsupportedNode {
            raw: serde_json::Value::Null,
            raw_format: None,
            reason: String::new(),
        }),
        endpoint: deve_sub_domain::Endpoint {
            host: Host::Domain(deve_sub_domain::DomainName::new(String::new())),
            port: 0,
        },
        authentication: deve_sub_domain::Authentication::None,
        transport: None,
        tls: None,
        udp: deve_sub_domain::UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: deve_sub_domain::NodeSource {
            source_label: String::new(),
            raw_uri: Some(raw_uri.to_owned()),
            imported_at: deve_sub_kernel::Timestamp::now(),
        },
        tags: vec![],
        region: deve_sub_domain::RegionAssignment {
            method: deve_sub_domain::RegionMethod::Auto,
            value: None,
        },
        extras: std::collections::BTreeMap::new(),
    }
}
