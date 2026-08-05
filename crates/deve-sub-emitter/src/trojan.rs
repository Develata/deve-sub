//! Trojan URI emitter.
//!
//! Serializes a canonical [`deve_sub_domain::Node`] with
//! `ProtocolKind::Trojan` + `ProtocolConfig::Trojan` back to a
//! `trojan://` share URI.
//!
//! ## Query parameter order
//!
//! `sni`, `alpn`, `skip-cert-verify`, `type`, `path`, `host`,
//! `packetEncoding`.

use deve_sub_domain::{Authentication, Node, ProtocolConfig, TransportKind, TrojanConfig};

use crate::common::{format_fragment, format_query};
use crate::error::EmitError;
use crate::transport::transport_kind_str;

/// Emit a Trojan [`Node`] as a `trojan://` share URI.
pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    let password = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(EmitError::MissingField("password authentication")),
    };

    let TrojanConfig { packet_encoding } = match &node.config {
        ProtocolConfig::Trojan(cfg) => cfg,
        _ => return Err(EmitError::NoEmitter("non-Trojan config".to_owned())),
    };

    let transport_kind = node
        .transport
        .as_ref()
        .map(|t| t.kind)
        .unwrap_or(TransportKind::Tcp);

    let tls = node.tls.as_ref();

    let mut params: Vec<(String, String)> = Vec::new();

    if let Some(tls) = tls {
        if let Some(ref sni) = tls.server_name {
            params.push(("sni".to_owned(), sni.clone()));
        }
        if !tls.alpn.is_empty() {
            params.push(("alpn".to_owned(), tls.alpn.join(",")));
        }
        if let Some(skip) = tls.skip_cert_verify {
            params.push((
                "skip-cert-verify".to_owned(),
                if skip { "1" } else { "0" }.to_owned(),
            ));
        }
    }

    params.push((
        "type".to_owned(),
        transport_kind_str(transport_kind).to_owned(),
    ));

    if let Some(ref transport) = node.transport {
        if let Some(ref path) = transport.path {
            params.push(("path".to_owned(), path.clone()));
        }
        if let Some(ref host) = transport.host {
            params.push(("host".to_owned(), host.clone()));
        }
    }

    if let Some(pe) = packet_encoding {
        params.push(("packetEncoding".to_owned(), pe.clone()));
    }

    let query = format_query(&params);

    let mut result = format!(
        "trojan://{password}@{host}:{port}?{query}",
        host = node.endpoint.host.uri_host(),
        port = node.endpoint.port,
    );
    if !node.display_name.is_empty() {
        result.push('#');
        result.push_str(&format_fragment(&node.display_name));
    }
    Ok(result)
}
