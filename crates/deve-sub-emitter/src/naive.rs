//! NaiveProxy URI emitter.
//!
//! Serializes a canonical [`deve_sub_domain::Node`] with
//! `ProtocolKind::NaiveProxy` + `ProtocolConfig::NaiveProxy` back to a
//! `naive+https://` or `naive+http://` share URI.
//!
//! WHY: NaiveProxy must not be downgraded to plain HTTP (PARSE-004). If the
//! node has TLS, the scheme is `naive+https://`; otherwise `naive+http://`.
//! The emitter preserves the original TLS state rather than forcing TLS.
//!
//! ## Query parameter order
//!
//! `sni`, `alpn`, `skip-cert-verify`, `pinSHA256`, `quic`, `http2`, `http3`.

use deve_sub_domain::{Authentication, NaiveProxyConfig, Node, ProtocolConfig};

use crate::common::{format_fragment, format_pins, format_query};
use crate::error::EmitError;

/// Emit a NaiveProxy [`Node`] as a `naive+https://` or `naive+http://` share
/// URI.
pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    let (username, password) = match &node.authentication {
        Authentication::UserPassword { username, password } => (username, password),
        _ => return Err(EmitError::MissingField("user+password authentication")),
    };

    let NaiveProxyConfig { quic, http2, http3 } = match &node.config {
        ProtocolConfig::NaiveProxy(cfg) => cfg,
        _ => return Err(EmitError::NoEmitter("non-NaiveProxy config".to_owned())),
    };

    let scheme = if node.tls.is_some() {
        "naive+https"
    } else {
        "naive+http"
    };

    let mut params: Vec<(String, String)> = Vec::new();

    if let Some(tls) = node.tls.as_ref() {
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
        if !tls.certificate_pins.is_empty() {
            params.push(("pinSHA256".to_owned(), format_pins(&tls.certificate_pins)));
        }
    }

    if let Some(q) = quic {
        params.push(("quic".to_owned(), if *q { "1" } else { "0" }.to_owned()));
    }
    if let Some(h2) = http2 {
        params.push(("http2".to_owned(), if *h2 { "1" } else { "0" }.to_owned()));
    }
    if let Some(h3) = http3 {
        params.push(("http3".to_owned(), if *h3 { "1" } else { "0" }.to_owned()));
    }

    let query = format_query(&params);

    let mut result = format!(
        "{scheme}://{username}:{password}@{host}:{port}?{query}",
        host = node.endpoint.host.uri_host(),
        port = node.endpoint.port,
    );
    if !node.display_name.is_empty() {
        result.push('#');
        result.push_str(&format_fragment(&node.display_name));
    }
    Ok(result)
}
