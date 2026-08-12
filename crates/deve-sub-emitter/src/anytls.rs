//! AnyTLS URI emitter.
//!
//! Serializes a canonical [`deve_sub_domain::Node`] with
//! `ProtocolKind::AnyTls` + `ProtocolConfig::AnyTls` back to an
//! `anytls://` share URI.
//!
//! ## Query parameter order
//!
//! `sni`, `alpn`, `insecure`, `fp`.

use deve_sub_domain::{AnyTlsConfig, Authentication, Node, ProtocolConfig};

use crate::common::{format_fragment, format_query};
use crate::error::EmitError;

pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    let password = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(EmitError::MissingField("anytls password authentication")),
    };

    let AnyTlsConfig {
        idle_session_check_interval: _,
        idle_session_timeout: _,
        min_idle_session: _,
        client_metadata: _,
    } = match &node.config {
        ProtocolConfig::AnyTls(c) => c,
        _ => return Err(EmitError::NoEmitter("non-AnyTLS config".to_owned())),
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
                "insecure".to_owned(),
                if skip { "1" } else { "0" }.to_owned(),
            ));
        }
        if let Some(ref fp) = tls.client_fingerprint {
            params.push(("fp".to_owned(), fp.clone()));
        }
    }

    let query = format_query(&params);

    let mut result = if params.is_empty() {
        format!(
            "anytls://{password}@{host}:{port}",
            host = node.endpoint.host.uri_host(),
            port = node.endpoint.port,
        )
    } else {
        format!(
            "anytls://{password}@{host}:{port}?{query}",
            host = node.endpoint.host.uri_host(),
            port = node.endpoint.port,
        )
    };
    if !node.display_name.is_empty() {
        result.push('#');
        result.push_str(&format_fragment(&node.display_name));
    }
    Ok(result)
}
