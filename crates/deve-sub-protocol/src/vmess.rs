//! VMess URI parser.
//!
//! Parses `vmess://BASE64(JSON)` URIs into canonical
//! [`deve_sub_domain::Node`] values. The VMess share format is fundamentally
//! different from other protocols: the entire configuration is a Base64-encoded
//! JSON object, not a standard URL with query parameters.
//!
//! ## JSON format
//!
//! ```json
//! {
//!   "v": "2", "ps": "name", "add": "host", "port": "443",
//!   "id": "uuid", "aid": "0", "scy": "auto", "net": "tcp",
//!   "type": "none", "host": "", "path": "", "tls": "tls",
//!   "sni": "", "alpn": ""
//! }
//! ```
//!
//! Note: `port` and `aid` may be strings or numbers depending on the
//! implementation. Both are accepted.

use serde::Deserialize;

use deve_sub_domain::{
    Authentication, Endpoint, Node, ProtocolConfig, ProtocolKind, TlsConfig, Transport,
    TransportKind, VMessConfig,
};

use crate::error::ParseError;
use crate::transport::map_transport_kind;
use crate::uri::{decode_base64_flexible, node_shell, parse_host};

/// Parsed VMess JSON body.
#[derive(Deserialize)]
struct VmessJson {
    #[allow(dead_code)]
    v: Option<String>,
    ps: Option<String>,
    add: String,
    port: serde_json::Value,
    id: String,
    aid: Option<serde_json::Value>,
    scy: Option<String>,
    net: Option<String>,
    #[serde(rename = "type")]
    header_type: Option<String>,
    host: Option<String>,
    path: Option<String>,
    tls: Option<String>,
    sni: Option<String>,
    alpn: Option<String>,
    #[serde(rename = "packetEncoding")]
    packet_encoding: Option<String>,
}

/// Parse a raw `vmess://BASE64(JSON)` URI into a canonical [`Node`].
pub(crate) fn parse(uri: &str) -> Result<Node, ParseError> {
    let body = uri
        .strip_prefix("vmess://")
        .ok_or(ParseError::UnknownScheme("expected vmess://".to_owned()))?;

    let decoded = decode_base64_flexible(body)?;
    let json_str =
        String::from_utf8(decoded).map_err(|e| ParseError::InvalidBase64(e.to_string()))?;

    let vmess: VmessJson =
        serde_json::from_str(&json_str).map_err(|e| ParseError::InvalidJson(e.to_string()))?;

    let port = json_value_as_u16(&vmess.port, "port")?;
    let alter_id = vmess
        .aid
        .as_ref()
        .map(|v| json_value_as_u32(v, "aid"))
        .transpose()?;

    let host = parse_host(&vmess.add)?;

    let transport_kind = vmess
        .net
        .as_deref()
        .map(map_transport_kind)
        .transpose()?
        .unwrap_or(TransportKind::Tcp);

    // WHY: Transport `host` and `path` are only meaningful for non-TCP
    // transports. Store them regardless; the emitter will include them only
    // when relevant.
    let transport = Some(Transport {
        kind: transport_kind,
        path: vmess.path.filter(|p| !p.is_empty()),
        host: vmess.host.filter(|h| !h.is_empty()),
    });

    let tls = vmess.tls.as_deref().filter(|t| !t.is_empty()).map(|t| {
        let alpn = vmess
            .alpn
            .as_deref()
            .filter(|a| !a.is_empty())
            .map(|a| a.split(',').map(String::from).collect())
            .unwrap_or_default();

        TlsConfig {
            enabled: t == "tls" || t == "reality" || t == "xtls",
            server_name: vmess.sni.filter(|s| !s.is_empty()),
            skip_cert_verify: None,
            alpn,
            client_fingerprint: None,
            certificate_pins: vec![],
            reality: None,
        }
    });

    // Header obfuscation type (`type` field) is not `none` for KCP. Store it
    // in extras since the canonical model has no field for it.
    let mut extras = std::collections::BTreeMap::new();
    if let Some(ht) = &vmess.header_type
        && !ht.is_empty()
        && ht != "none"
    {
        extras.insert(
            "vmess_header_type".to_owned(),
            serde_json::Value::String(ht.clone()),
        );
    }

    let config = ProtocolConfig::VMess(VMessConfig {
        alter_id,
        security: vmess.scy.filter(|s| !s.is_empty()),
        packet_encoding: vmess.packet_encoding.filter(|s| !s.is_empty()),
    });

    let mut node = node_shell(uri);
    node.display_name = vmess.ps.unwrap_or_default();
    node.protocol = ProtocolKind::VMess;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::Uuid { uuid: vmess.id };
    node.transport = transport;
    node.tls = tls;
    node.extras = extras;

    Ok(node)
}

/// Convert a JSON value (string or number) to a `u16` port.
fn json_value_as_u16(value: &serde_json::Value, field: &'static str) -> Result<u16, ParseError> {
    match value {
        serde_json::Value::String(s) => s.parse().map_err(|_| ParseError::InvalidField {
            field,
            value: s.clone(),
        }),
        serde_json::Value::Number(n) => {
            n.as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .ok_or_else(|| ParseError::InvalidField {
                    field,
                    value: n.to_string(),
                })
        }
        _ => Err(ParseError::MissingField(field)),
    }
}

/// Convert a JSON value (string or number) to a `u32` alter ID.
fn json_value_as_u32(value: &serde_json::Value, field: &'static str) -> Result<u32, ParseError> {
    match value {
        serde_json::Value::String(s) => s.parse().map_err(|_| ParseError::InvalidField {
            field,
            value: s.clone(),
        }),
        serde_json::Value::Number(n) => {
            n.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| ParseError::InvalidField {
                    field,
                    value: n.to_string(),
                })
        }
        _ => Err(ParseError::MissingField(field)),
    }
}
