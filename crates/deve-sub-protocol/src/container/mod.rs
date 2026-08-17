//! Container format parsers: Base64 subscription, URI list, Mihomo YAML,
//! sing-box JSON, Xray/V2Ray JSON, Shadowrocket.
//!
//! Each parser maps a container format to a `Vec<Node>`. Unsupported
//! protocols are preserved as `ProtocolConfig::Unsupported` (constraint #7:
//! no silent dropping of incompatible nodes). Only structural errors (invalid
//! YAML/JSON, missing top-level key) abort the entire parse; entry-level
//! issues produce `UnsupportedNode` entries.
//!
//! See `docs/plan/05-protocol-engine.md` §"Input formats vs protocols" and
//! `docs/plan/milestones/M3-protocol-engine.md` Slice 3.

pub mod base64;
pub mod mihomo;
pub mod shadowrocket;
pub mod singbox;
pub mod uri_list;
pub mod xray_v2ray;

pub use base64::parse_base64_subscription;
pub use mihomo::parse_mihomo_yaml;
pub use shadowrocket::parse_shadowrocket;
pub use singbox::parse_singbox_json;
pub use uri_list::parse_uri_list;
pub use xray_v2ray::{parse_v2ray_json, parse_xray_json};

use serde_json::Value;

use deve_sub_domain::{Host, Node, ProtocolConfig, ProtocolKind, TlsConfig, UnsupportedNode};

use crate::error::ParseError;

/// Extract an optional string field from a JSON value.
pub(crate) fn get_str(v: &Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(String::from)
}

/// Extract an optional boolean field from a JSON value. Handles both JSON
/// bools and common string encodings (`"true"`, `"false"`, `"1"`, `"0"`).
pub(crate) fn get_bool(v: &Value, key: &str) -> Option<bool> {
    let val = v.get(key)?;
    if let Some(b) = val.as_bool() {
        return Some(b);
    }
    match val.as_str()? {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// Extract a port from a JSON value. Handles both numeric and string ports.
pub(crate) fn get_port(v: &Value, key: &str) -> Result<Option<u16>, ParseError> {
    let Some(port_val) = v.get(key) else {
        return Ok(None);
    };
    if port_val.is_null() {
        return Ok(None);
    }
    if let Some(n) = port_val.as_u64() {
        return u16::try_from(n)
            .map(Some)
            .map_err(|_| ParseError::InvalidPort(n.to_string()));
    }
    if let Some(s) = port_val.as_str() {
        return s
            .parse::<u16>()
            .map(Some)
            .map_err(|_| ParseError::InvalidPort(s.to_owned()));
    }
    Ok(None)
}

/// Parse a host string into a [`Host`], reusing the URI parser's logic.
pub(crate) fn parse_host_str(s: &str) -> Result<Host, ParseError> {
    crate::uri::parse_host(s)
}

/// Create an [`Node`] shell for container format entries (no `raw_uri`).
pub(crate) fn node_shell_container() -> Node {
    crate::uri::node_shell(None)
}

/// Default TLS config for protocols that always use TLS (Trojan, Hysteria2,
/// TUIC, NaiveProxy). Used when no TLS-related fields are present in the
/// container entry.
pub(crate) fn default_tls_enabled() -> TlsConfig {
    TlsConfig {
        enabled: true,
        server_name: None,
        skip_cert_verify: None,
        alpn: vec![],
        client_fingerprint: None,
        certificate_pins: vec![],
        reality: None,
    }
}

/// Extract a string field that may appear as a single string or a
/// single-element array. Mihomo's `h2-opts.host` is a list in the official
/// format, but some configs use a bare string; this helper accepts both.
pub(crate) fn get_str_or_first(v: &Value, key: &str) -> Option<String> {
    let val = v.get(key)?;
    if let Some(s) = val.as_str() {
        return Some(s.to_owned());
    }
    val.as_array()
        .and_then(|arr| arr.first())
        .and_then(|e| e.as_str())
        .map(String::from)
}

/// Extract a string array field from a JSON value.
pub(crate) fn get_str_array(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Create an `UnsupportedNode` from a container entry, preserving the raw
/// data and recording the reason.
pub(crate) fn unsupported_entry(
    raw: &Value,
    raw_format: &str,
    protocol: ProtocolKind,
    reason: String,
) -> Node {
    let mut node = node_shell_container();
    node.protocol = protocol;
    node.config = ProtocolConfig::Unsupported(UnsupportedNode {
        raw: raw.clone(),
        raw_format: Some(raw_format.to_owned()),
        reason,
    });
    node
}
