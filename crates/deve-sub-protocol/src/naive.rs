//! NaiveProxy URI parser.
//!
//! Parses `naive+https://` and `naive+http://` URIs into canonical
//! [`deve_sub_domain::Node`] values. The `+https` suffix indicates TLS;
//! Naive must not be downgraded to a plain HTTP node (PARSE-004).
//!
//! ## URI format
//!
//! ```text
//! naive+https://<username>:<password>@<host>:<port>?sni=...&alpn=...
//!   &quic=1&http2=1&http3=0&pinSHA256=...&skip-cert-verify=0#<display_name>
//! ```

use std::collections::HashMap;

use deve_sub_domain::{
    Authentication, Endpoint, NaiveProxyConfig, Node, ProtocolConfig, ProtocolKind, TlsConfig,
};

use crate::error::ParseError;
use crate::uri::{
    build_common_tls, collect_query, decode_fragment, node_shell, parse_bool, parse_host,
};

/// Parse a parsed `naive+https://` or `naive+http://` URL into a canonical
/// [`Node`].
pub(crate) fn parse(url: &url::Url, raw_uri: &str) -> Result<Node, ParseError> {
    let username = url.username();
    if username.is_empty() {
        return Err(ParseError::MissingField("username"));
    }
    let password = url.password().ok_or(ParseError::MissingField("password"))?;

    let host_str = url
        .host_str()
        .ok_or_else(|| ParseError::InvalidHost("missing host in naive URI".to_owned()))?;
    let host = parse_host(host_str)?;

    let port = url.port().ok_or(ParseError::MissingField("port"))?;

    let display_name = decode_fragment(url);
    let query: HashMap<String, String> = collect_query(url);

    let is_https = url.scheme() == "naive+https";

    let tls = if is_https {
        // WHY: naive+https always uses TLS. Even without explicit TLS query
        // params, the scheme guarantees TLS is enabled.
        Some(
            build_common_tls(&query, &["skip-cert-verify", "insecure"], Some("pinSHA256"))?
                .unwrap_or_else(|| TlsConfig {
                    enabled: true,
                    server_name: None,
                    skip_cert_verify: None,
                    alpn: vec![],
                    client_fingerprint: None,
                    certificate_pins: vec![],
                    reality: None,
                }),
        )
    } else {
        // naive+http — no TLS.
        None
    };

    let quic = query.get("quic").map(|v| parse_bool(v)).transpose()?;
    let http2 = query.get("http2").map(|v| parse_bool(v)).transpose()?;
    let http3 = query.get("http3").map(|v| parse_bool(v)).transpose()?;

    let config = ProtocolConfig::NaiveProxy(NaiveProxyConfig { quic, http2, http3 });

    let mut node = node_shell(raw_uri);
    node.display_name = display_name;
    node.protocol = ProtocolKind::NaiveProxy;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::UserPassword {
        username: username.to_owned(),
        password: password.to_owned(),
    };
    node.tls = tls;

    Ok(node)
}
