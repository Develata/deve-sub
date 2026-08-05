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
    Transport, TransportKind, UdpCapability, VlessRealityConfig,
};

use crate::error::ParseError;
use crate::uri::{node_shell, parse_bool, parse_host};

/// Parse a parsed `vless://` URL into a canonical [`Node`].
pub(crate) fn parse(url: &url::Url, raw_uri: &str) -> Result<Node, ParseError> {
    let uuid = url.username();
    if uuid.is_empty() {
        return Err(ParseError::MissingField("uuid"));
    }

    let host_str = url
        .host_str()
        .ok_or_else(|| ParseError::InvalidHost("missing host in vless URI".to_owned()))?;
    let host = parse_host(host_str)?;

    let port = url.port().ok_or(ParseError::MissingField("port"))?;

    // WHY: url::Url::fragment() returns the percent-encoded fragment. Decode
    // it here so the canonical Node stores the raw display name; the emitter
    // re-encodes it, avoiding double-encoding on round-trip.
    let display_name = url
        .fragment()
        .map(|f| {
            percent_encoding::percent_decode_str(f)
                .decode_utf8_lossy()
                .into_owned()
        })
        .unwrap_or_default();

    let query: HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let security = query.get("security").map(String::as_str).unwrap_or("");
    let is_reality = security == "reality";

    let transport_kind = query
        .get("type")
        .map(|t| map_transport_kind(t))
        .transpose()?
        .unwrap_or(TransportKind::Tcp);

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

    let mut node = node_shell(raw_uri);
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
    });
    node.tls = tls;
    node.udp = udp;

    Ok(node)
}

/// Map the `type` query parameter to a [`TransportKind`].
fn map_transport_kind(value: &str) -> Result<TransportKind, ParseError> {
    match value {
        "tcp" => Ok(TransportKind::Tcp),
        "ws" => Ok(TransportKind::Ws),
        "grpc" => Ok(TransportKind::Grpc),
        "h2" => Ok(TransportKind::H2),
        "kcp" => Ok(TransportKind::Kcp),
        "quic" => Ok(TransportKind::Quic),
        "httpupgrade" => Ok(TransportKind::HttpUpgrade),
        "xtls" => Ok(TransportKind::Xtls),
        _ => Err(ParseError::InvalidField {
            field: "type (transport)",
            value: value.to_owned(),
        }),
    }
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

    let skip_cert_verify = match query.get("allowInsecure").map(String::as_str) {
        None => None,
        Some("0") => Some(false),
        Some("1") => Some(true),
        Some(v) => {
            return Err(ParseError::InvalidField {
                field: "allowInsecure",
                value: v.to_owned(),
            });
        }
    };

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

    let alpn = query
        .get("alpn")
        .map(|v| {
            if v.is_empty() {
                vec![]
            } else {
                v.split(',').map(String::from).collect()
            }
        })
        .unwrap_or_default();

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
