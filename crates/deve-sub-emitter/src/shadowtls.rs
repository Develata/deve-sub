//! ShadowTLS URI emitter.
//!
//! Serializes a canonical [`deve_sub_domain::Node`] with
//! `ProtocolKind::ShadowTls` + `ProtocolConfig::ShadowTls` back to a
//! `shadow-tls://` share URI. There is no official ShadowTLS URI scheme;
//! Deve Sub emits the de-facto format (see ADR-0007 / M9 Slice 4).
//!
//! ## Query parameter order
//!
//! `version`, `sni`, `insecure`.
//!
//! ## Password source
//!
//! The URI userinfo carries the ShadowTLS *wrapper* password
//! (`ShadowTlsConfig.password`), NOT `node.authentication`. Container
//! parsers set `node.authentication` to the inner protocol's password
//! (e.g. trojan password), which is not representable in the URI format.
//! Using `cfg.password` keeps URI round-trip self-consistent regardless
//! of whether the node was parsed from a URI or a container config.
//!
//! The URI format cannot represent the inner protocol; nodes with
//! `inner_protocol = Unknown` (parsed from `shadow-tls://` URIs) round-trip
//! lossily by design — only the wrapper fields survive.

use deve_sub_domain::{Node, ProtocolConfig};

use crate::common::{format_fragment, format_query};
use crate::error::EmitError;

pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    let cfg = match &node.config {
        ProtocolConfig::ShadowTls(c) => c,
        _ => return Err(EmitError::NoEmitter("non-ShadowTls config".to_owned())),
    };

    // WHY: use the wrapper password from the typed config, not
    // `node.authentication`. Container parsers populate
    // `node.authentication` with the inner protocol's password (trojan/ss),
    // which is not the ShadowTLS wrapper password and would corrupt the
    // URI userinfo on round-trip. `cfg.password` is None only for V1,
    // which has no password — emit empty userinfo in that case.
    let password = cfg.password.as_deref().unwrap_or("");

    let mut params: Vec<(String, String)> = Vec::new();
    params.push(("version".to_owned(), cfg.version.as_u32().to_string()));

    if let Some(ref tls) = node.tls {
        if let Some(ref sni) = tls.server_name {
            params.push(("sni".to_owned(), sni.clone()));
        }
        if let Some(true) = tls.skip_cert_verify {
            params.push(("insecure".to_owned(), "1".to_owned()));
        }
    }

    let query = format_query(&params);

    let mut result = format!(
        "shadow-tls://{password}@{host}:{port}?{query}",
        host = node.endpoint.host.uri_host(),
        port = node.endpoint.port,
    );
    if !node.display_name.is_empty() {
        result.push('#');
        result.push_str(&format_fragment(&node.display_name));
    }
    Ok(result)
}
