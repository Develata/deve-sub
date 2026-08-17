//! AnyTLS URI parser.
//!
//! Parses `anytls://` URIs into canonical [`deve_sub_domain::Node`] values.
//! AnyTLS always requires TLS; `node.tls` is always `Some`.
//!
//! ## URI format
//!
//! ```text
//! anytls://<password>@<host>:<port>?sni=...&insecure=0|1&alpn=...&fp=...#<name>
//! ```
//!
//! Default port is 443. `insecure=1` maps to `tls.skip_cert_verify = Some(true)`.
//! The idle-session tuning and `client_metadata` fields are sing-box/mihomo
//! extensions with no URI representation; they are populated only by container
//! parsers.

use std::collections::HashMap;

use deve_sub_domain::{
    AnyTlsConfig, Authentication, Endpoint, Node, ProtocolConfig, ProtocolKind, TlsConfig,
};

use crate::error::ParseError;
use crate::uri::{
    build_common_tls, collect_query, decode_fragment, decode_userinfo, node_shell, parse_host,
};

pub(crate) fn parse(url: &url::Url, raw_uri: &str) -> Result<Node, ParseError> {
    let password = decode_userinfo(url.username());
    if password.is_empty() {
        return Err(ParseError::MissingField("password"));
    }

    let host_str = url
        .host_str()
        .ok_or_else(|| ParseError::InvalidHost("missing host in anytls URI".to_owned()))?;
    let host = parse_host(host_str)?;

    let port = url.port().ok_or(ParseError::MissingField("port"))?;

    let display_name = decode_fragment(url);
    let query: HashMap<String, String> = collect_query(url);

    // WHY: AnyTLS always requires TLS; the `tls` field is unconditionally
    // Some even when no TLS-related query params are present. `build_common_tls`
    // does not handle the `fp` (client_fingerprint) query param, so merge it
    // in after.
    let mut tls = build_common_tls(
        &query,
        &["insecure", "skip-cert-verify", "allowInsecure"],
        None,
    )?
    .unwrap_or_else(|| TlsConfig {
        enabled: true,
        server_name: None,
        skip_cert_verify: None,
        alpn: vec![],
        client_fingerprint: None,
        certificate_pins: vec![],
        reality: None,
    });
    if tls.client_fingerprint.is_none() {
        tls.client_fingerprint = query.get("fp").cloned();
    }

    let config = ProtocolConfig::AnyTls(AnyTlsConfig {
        idle_session_check_interval: None,
        idle_session_timeout: None,
        min_idle_session: None,
        client_metadata: None,
    });

    let mut node = node_shell(Some(raw_uri));
    node.display_name = display_name;
    node.protocol = ProtocolKind::AnyTls;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::Password {
        password: password.to_owned(),
    };
    node.tls = Some(tls);

    Ok(node)
}
