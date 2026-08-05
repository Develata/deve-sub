//! VMess URI emitter.
//!
//! Serializes a canonical [`deve_sub_domain::Node`] with
//! `ProtocolKind::VMess` + `ProtocolConfig::VMess` back to a
//! `vmess://BASE64(JSON)` share URI.
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

use base64::Engine;

use deve_sub_domain::{Authentication, Host, Node, ProtocolConfig, TransportKind, VMessConfig};

use crate::error::EmitError;
use crate::transport::transport_kind_str;

/// Render a [`Host`] as a plain string (no IPv6 brackets) for the VMess JSON
/// `add` field.
fn host_to_string(host: &Host) -> String {
    match host {
        Host::Ipv4(addr) => addr.to_string(),
        Host::Ipv6(addr) => addr.to_string(),
        Host::Domain(d) => d.to_string(),
    }
}

/// Emit a VMess [`Node`] as a `vmess://BASE64(JSON)` share URI.
pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    let uuid = match &node.authentication {
        Authentication::Uuid { uuid } => uuid,
        _ => return Err(EmitError::MissingField("uuid authentication")),
    };

    let VMessConfig {
        alter_id,
        security,
        packet_encoding,
    } = match &node.config {
        ProtocolConfig::VMess(cfg) => cfg,
        _ => return Err(EmitError::NoEmitter("non-VMess config".to_owned())),
    };

    let transport_kind = node
        .transport
        .as_ref()
        .map(|t| t.kind)
        .unwrap_or(TransportKind::Tcp);

    let net = transport_kind_str(transport_kind);

    let (path, host) = node
        .transport
        .as_ref()
        .map(|t| {
            (
                t.path.clone().unwrap_or_default(),
                t.host.clone().unwrap_or_default(),
            )
        })
        .unwrap_or_default();

    let (tls_str, sni, alpn) = node
        .tls
        .as_ref()
        .map(|tls| {
            let tls_str = if tls.enabled { "tls" } else { "" };
            let sni = tls.server_name.clone().unwrap_or_default();
            let alpn = if tls.alpn.is_empty() {
                String::new()
            } else {
                tls.alpn.join(",")
            };
            (tls_str.to_owned(), sni, alpn)
        })
        .unwrap_or_default();

    let header_type = node
        .extras
        .get("vmess_header_type")
        .and_then(|v| v.as_str())
        .unwrap_or("none")
        .to_owned();

    let json_body = serde_json::json!({
        "v": "2",
        "ps": node.display_name,
        "add": host_to_string(&node.endpoint.host),
        "port": node.endpoint.port.to_string(),
        "id": uuid,
        "aid": alter_id.unwrap_or(0).to_string(),
        "scy": security.clone().unwrap_or_else(|| "auto".to_owned()),
        "net": net,
        "type": header_type,
        "host": host,
        "path": path,
        "tls": tls_str,
        "sni": sni,
        "alpn": alpn,
        "packetEncoding": packet_encoding.clone().unwrap_or_default(),
    });

    let json_str = serde_json::to_string(&json_body).map_err(|e| EmitError::InvalidField {
        field: "vmess json",
        value: e.to_string(),
    })?;

    let encoded = base64::engine::general_purpose::STANDARD.encode(json_str.as_bytes());

    Ok(format!("vmess://{encoded}"))
}
