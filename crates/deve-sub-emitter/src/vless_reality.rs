//! VLESS Reality URI emitter.
//!
//! Serializes a canonical [`deve_sub_domain::Node`] with
//! `ProtocolKind::Vless` + `ProtocolConfig::VlessReality` back to a
//! `vless://` share URI. Non-Reality VLESS nodes (`ProtocolConfig::Unsupported`)
//! have no P0 emitter and return [`EmitError::NoEmitter`].
//!
//! ## Query parameter order
//!
//! Parameters are emitted in a fixed order for deterministic golden tests:
//! `security`, `type`, `allowInsecure`, `sni`, `fp`, `flow`, `sid`, `pbk`,
//! `encryption`, `packetEncoding`, `udp`, `xudp`, `spx`, `path`, `host`,
//! `alpn`.

use deve_sub_domain::{Authentication, Node, ProtocolConfig, TransportKind, VlessRealityConfig};

use crate::common::{format_fragment, format_query};
use crate::error::EmitError;
use crate::transport::transport_kind_str;

/// Emit a VLESS Reality [`Node`] as a `vless://` share URI.
pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    let uuid = match &node.authentication {
        Authentication::Uuid { uuid } => uuid,
        _ => return Err(EmitError::MissingField("uuid authentication")),
    };

    let VlessRealityConfig {
        encryption,
        flow,
        packet_encoding,
    } = match &node.config {
        ProtocolConfig::VlessReality(cfg) => cfg,
        ProtocolConfig::Unsupported(_) => {
            return Err(EmitError::NoEmitter("VLESS non-Reality".to_owned()));
        }
        _ => return Err(EmitError::NoEmitter("non-VLESS config".to_owned())),
    };

    let transport_kind = node
        .transport
        .as_ref()
        .map(|t| t.kind)
        .unwrap_or(TransportKind::Tcp);

    let tls = node.tls.as_ref();

    // WHY: security=reality is emitted unconditionally below. Without
    // tls.reality the emitted URI lacks sid/pbk and is un-reparseable.
    if tls.is_none_or(|t| t.reality.is_none()) {
        return Err(EmitError::MissingField("tls.reality"));
    }

    let mut params: Vec<(String, String)> = Vec::new();

    params.push(("security".to_owned(), "reality".to_owned()));
    params.push((
        "type".to_owned(),
        transport_kind_str(transport_kind).to_owned(),
    ));

    if let Some(tls) = tls {
        if let Some(skip) = tls.skip_cert_verify {
            params.push((
                "allowInsecure".to_owned(),
                if skip { "1" } else { "0" }.to_owned(),
            ));
        }
        if let Some(ref sni) = tls.server_name {
            params.push(("sni".to_owned(), sni.clone()));
        }
        if let Some(ref fp) = tls.client_fingerprint {
            params.push(("fp".to_owned(), fp.clone()));
        }
    }

    if let Some(f) = flow {
        params.push(("flow".to_owned(), f.clone()));
    }

    if let Some(tls) = tls
        && let Some(reality) = &tls.reality
    {
        params.push(("sid".to_owned(), reality.short_id.clone()));
        params.push(("pbk".to_owned(), reality.public_key.clone()));
    }

    if let Some(enc) = encryption {
        params.push(("encryption".to_owned(), enc.clone()));
    }
    if let Some(pe) = packet_encoding {
        params.push(("packetEncoding".to_owned(), pe.clone()));
    }

    if let Some(udp) = node.udp.supported {
        params.push((
            "udp".to_owned(),
            if udp { "true" } else { "false" }.to_owned(),
        ));
    }
    if let Some(xudp) = node.udp.xudp {
        params.push((
            "xudp".to_owned(),
            if xudp { "true" } else { "false" }.to_owned(),
        ));
    }

    if let Some(tls) = tls
        && let Some(reality) = &tls.reality
        && let Some(spx) = &reality.spider_x
    {
        params.push(("spx".to_owned(), spx.clone()));
    }

    if let Some(ref transport) = node.transport {
        if let Some(ref path) = transport.path {
            params.push(("path".to_owned(), path.clone()));
        }
        if let Some(ref host) = transport.host {
            params.push(("host".to_owned(), host.clone()));
        }
    }

    if let Some(tls) = tls
        && !tls.alpn.is_empty()
    {
        params.push(("alpn".to_owned(), tls.alpn.join(",")));
    }

    let query = format_query(&params);

    let mut result = format!(
        "vless://{uuid}@{host}:{port}?{query}",
        host = node.endpoint.host.uri_host(),
        port = node.endpoint.port,
    );
    if !node.display_name.is_empty() {
        result.push('#');
        result.push_str(&format_fragment(&node.display_name));
    }
    Ok(result)
}
