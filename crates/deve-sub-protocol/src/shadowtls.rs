//! ShadowTLS URI parser.
//!
//! Parses `shadow-tls://` URIs into canonical [`deve_sub_domain::Node`]
//! values. There is no official ShadowTLS URI scheme; Deve Sub parses and
//! emits the de-facto format (see ADR-0007 / M9 Slice 4).
//!
//! ## URI format
//!
//! ```text
//! shadow-tls://<password>@<host>:<port>?version=<1-3>&sni=...#<display_name>
//! ```
//!
//! The URI carries only the ShadowTLS wrapper parameters (version, password,
//! camouflage SNI). The inner protocol is not representable in this URI
//! format — `inner_protocol` defaults to `Unknown` and `inner_config` to
//! `Unsupported`. Container parsers (sing-box, mihomo) populate the inner
//! protocol when parsing a full config; URI round-trip preserves only the
//! wrapper fields.

use std::collections::HashMap;

use deve_sub_domain::{
    Authentication, Endpoint, Node, ProtocolConfig, ProtocolKind, ShadowTlsConfig,
    ShadowTlsVersion, TlsConfig,
};

use crate::error::ParseError;
use crate::uri::{
    collect_query, decode_fragment, decode_userinfo, node_shell, parse_host, query_insecure,
};

pub(crate) fn parse(url: &url::Url, raw_uri: &str) -> Result<Node, ParseError> {
    let password = decode_userinfo(url.username());

    let host_str = url
        .host_str()
        .ok_or_else(|| ParseError::InvalidHost("missing host in shadow-tls URI".to_owned()))?;
    let host = parse_host(host_str)?;

    let port = url.port().ok_or(ParseError::MissingField("port"))?;

    let display_name = decode_fragment(url);
    let query: HashMap<String, String> = collect_query(url);

    let version_n = query
        .get("version")
        .ok_or(ParseError::MissingField("version"))
        .and_then(|v| {
            v.parse::<u32>().map_err(|_| ParseError::InvalidField {
                field: "version",
                value: v.clone(),
            })
        })?;
    let version = ShadowTlsVersion::from_u32(version_n).ok_or(ParseError::InvalidField {
        field: "version",
        value: version_n.to_string(),
    })?;

    // WHY: V1 has no wrapper password (the protocol does not authenticate
    // the handshake), so the emitter emits empty userinfo for V1. Requiring
    // a non-empty password here made V1 output unparseable. V2/V3 require
    // the wrapper password. See R3-15.
    let password = if password.is_empty() {
        if matches!(version, ShadowTlsVersion::V1) {
            None
        } else {
            return Err(ParseError::MissingField("password"));
        }
    } else {
        Some(password)
    };

    // WHY: ShadowTLS camouflage TLS — the `sni` query param is the
    // camouflage server name for the TLS handshake target. ShadowTLS
    // always has a TLS layer (the handshake is the whole point), so
    // `node.tls` is unconditionally `Some` with `enabled: true`.
    let sni = query.get("sni").cloned();
    // WHY: route through the shared `query_insecure` helper so invalid
    // values (e.g. `insecure=maybe`) surface as `InvalidField` instead of
    // being silently mapped to `false`, matching every other boolean-query
    // parser (SRC-018).
    let insecure = query_insecure(&query, &["insecure"])?.unwrap_or(false);
    let tls = TlsConfig {
        enabled: true,
        server_name: sni,
        skip_cert_verify: Some(insecure),
        alpn: vec![],
        client_fingerprint: None,
        certificate_pins: vec![],
        reality: None,
    };

    let config = ProtocolConfig::ShadowTls(ShadowTlsConfig {
        version,
        password,
        // WHY: URI format cannot represent the inner protocol; defaults are
        // placeholders. Container parsers populate these when available.
        inner_protocol: ProtocolKind::Unknown(String::new()),
        inner_config: Box::new(ProtocolConfig::Unsupported(
            deve_sub_domain::UnsupportedNode {
                raw: serde_json::Value::Null,
                raw_format: None,
                reason: "shadow-tls URI: inner protocol not representable".to_owned(),
            },
        )),
    });

    let mut node = node_shell(Some(raw_uri));
    node.display_name = display_name;
    node.protocol = ProtocolKind::ShadowTls;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    // WHY: `node.authentication` carries the inner protocol's auth, not the
    // ShadowTLS wrapper password. The URI format has no inner protocol, so
    // auth is `None`. The wrapper password lives only in `cfg.password`.
    node.authentication = Authentication::None;
    node.tls = Some(tls);

    Ok(node)
}
