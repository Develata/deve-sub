//! VLESS Reality URI parser.
//!
//! Parses `vless://` URIs with `security=reality` into canonical
//! [`deve_sub_domain::Node`] values. Non-Reality VLESS URIs are preserved as
//! `ProtocolConfig::Unsupported` (P0 scopes VLESS to Reality only; see
//! ADR-0003).
//!
//! ## URI format
//!
//! ```text
//! vless://<uuid>@<host>:<port>?security=reality&type=tcp&sni=...&fp=...
//!   &flow=...&sid=...&pbk=...&encryption=none&allowInsecure=0
//!   &packetEncoding=...&udp=true&xudp=true&spx=...#<display_name>
//! ```

use std::collections::HashMap;

use deve_sub_domain::{
    Authentication, Endpoint, Node, ProtocolConfig, ProtocolKind, RealityConfig, TlsConfig,
    Transport, TransportKind, UdpCapability, VlessRealityConfig, XhttpMode,
};

use crate::error::ParseError;
use crate::transport::map_transport_kind;
use crate::uri::{
    collect_query, decode_fragment, decode_userinfo, node_shell, parse_alpn, parse_bool,
    parse_host, query_insecure,
};

/// Parse a parsed `vless://` URL into a canonical [`Node`].
pub(crate) fn parse(url: &url::Url, raw_uri: &str) -> Result<Node, ParseError> {
    let uuid = decode_userinfo(url.username());
    if uuid.is_empty() {
        return Err(ParseError::MissingField("uuid"));
    }

    let host_str = url
        .host_str()
        .ok_or_else(|| ParseError::InvalidHost("missing host in vless URI".to_owned()))?;
    let host = parse_host(host_str)?;

    let port = url.port().ok_or(ParseError::MissingField("port"))?;

    let display_name = decode_fragment(url);

    let query: HashMap<String, String> = collect_query(url);

    let security = query.get("security").map(String::as_str).unwrap_or("");
    let is_reality = security == "reality";

    let transport_kind = query
        .get("type")
        .map(|t| map_transport_kind(t))
        .transpose()?
        .unwrap_or(TransportKind::Tcp);

    let xhttp_mode = if transport_kind == TransportKind::Xhttp {
        query
            .get("mode")
            .map(|m| XhttpMode::from_str_lossy(m))
            .unwrap_or(Some(XhttpMode::Auto))
    } else {
        None
    };

    let tls = build_tls(&query, is_reality)?;

    let config = if is_reality {
        ProtocolConfig::VlessReality(VlessRealityConfig {
            encryption: query.get("encryption").cloned(),
            flow: query.get("flow").cloned(),
            packet_encoding: query.get("packetEncoding").cloned(),
        })
    } else {
        ProtocolConfig::Unsupported(deve_sub_domain::UnsupportedNode {
            raw: serde_json::Value::String(raw_uri.to_owned()),
            raw_format: Some("vless-uri".to_owned()),
            reason: format!("VLESS with security={security} is not P0 (Reality only)"),
        })
    };

    let udp = UdpCapability {
        supported: query.get("udp").map(|v| parse_bool(v)).transpose()?,
        xudp: query.get("xudp").map(|v| parse_bool(v)).transpose()?,
    };

    let mut node = node_shell(Some(raw_uri));
    node.display_name = display_name;
    node.protocol = ProtocolKind::Vless;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::Uuid {
        uuid: uuid.to_owned(),
    };
    node.transport = Some(Transport {
        kind: transport_kind,
        path: query.get("path").cloned(),
        host: query.get("host").cloned(),
        xhttp_mode,
    });
    node.tls = tls;
    node.udp = udp;

    Ok(node)
}

/// Build the [`TlsConfig`] from query parameters.
///
/// For Reality, the `reality` field is populated with `pbk`, `sid`, and
/// `spx`. The `allowInsecure` parameter maps to the three-state
/// `skip_cert_verify`.
fn build_tls(
    query: &HashMap<String, String>,
    is_reality: bool,
) -> Result<Option<TlsConfig>, ParseError> {
    let has_tls_params = is_reality
        || query.contains_key("sni")
        || query.contains_key("allowInsecure")
        || query.contains_key("fp")
        || query.contains_key("alpn");

    if !has_tls_params {
        return Ok(None);
    }

    let skip_cert_verify = query_insecure(query, &["allowInsecure"])?;

    let reality = if is_reality {
        let pbk = query
            .get("pbk")
            .ok_or(ParseError::MissingField("pbk (reality public key)"))?;
        let sid = query
            .get("sid")
            .ok_or(ParseError::MissingField("sid (reality short id)"))?;
        Some(RealityConfig {
            public_key: pbk.clone(),
            short_id: sid.clone(),
            spider_x: query.get("spx").cloned(),
        })
    } else {
        None
    };

    let alpn = query.get("alpn").map(|v| parse_alpn(v)).unwrap_or_default();

    Ok(Some(TlsConfig {
        enabled: true,
        server_name: query.get("sni").cloned(),
        skip_cert_verify,
        alpn,
        client_fingerprint: query.get("fp").cloned(),
        certificate_pins: vec![],
        reality,
    }))
}
