//! Snell URI emitter.
//!
//! Serializes a canonical [`deve_sub_domain::Node`] with
//! `ProtocolKind::Snell` + `ProtocolConfig::Snell` back to a `snell://`
//! share URI. There is no official Snell URI scheme; Deve Sub emits the
//! de-facto sublinkPro format (see ADR-0007 / M9 Slice 3).
//!
//! ## Query parameter order
//!
//! `version`, `udp`, `reuse`, `obfs`, `obfs-host`, `alpn`.

use deve_sub_domain::{Authentication, Node, ProtocolConfig, SnellConfig, SnellObfsMode};

use crate::common::{encode_userinfo, format_fragment, format_query};
use crate::error::EmitError;

pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    let psk = match &node.authentication {
        Authentication::Password { password } => password,
        _ => {
            return Err(EmitError::MissingField(
                "snell psk (password) authentication",
            ));
        }
    };

    let SnellConfig {
        version,
        reuse,
        obfs,
        v6_mode: _,
    } = match &node.config {
        ProtocolConfig::Snell(c) => c,
        _ => return Err(EmitError::NoEmitter("non-Snell config".to_owned())),
    };

    let mut params: Vec<(String, String)> = Vec::new();
    params.push(("version".to_owned(), version.as_u32().to_string()));

    if let Some(supported) = node.udp.supported {
        params.push((
            "udp".to_owned(),
            if supported { "1" } else { "0" }.to_owned(),
        ));
    }

    if let Some(reuse) = reuse {
        params.push((
            "reuse".to_owned(),
            if *reuse { "1" } else { "0" }.to_owned(),
        ));
    }

    if let Some(obfs) = obfs {
        let mode_str = match obfs.mode {
            SnellObfsMode::Http => "http",
            SnellObfsMode::Tls => "tls",
            SnellObfsMode::ShadowTls | SnellObfsMode::Restls | SnellObfsMode::Jls => {
                return Err(EmitError::NoEmitter(format!(
                    "snell obfs mode {:?} has no URI representation",
                    obfs.mode
                )));
            }
        };
        params.push(("obfs".to_owned(), mode_str.to_owned()));
        if let Some(ref host) = obfs.host {
            params.push(("obfs-host".to_owned(), host.clone()));
        }
        if !obfs.alpn.is_empty() {
            params.push(("alpn".to_owned(), obfs.alpn.join(",")));
        }
    }

    let query = format_query(&params);

    let psk_enc = encode_userinfo(psk);
    let mut result = format!(
        "snell://{psk_enc}@{host}:{port}?{query}",
        host = node.endpoint.host.uri_host(),
        port = node.endpoint.port,
    );
    if !node.display_name.is_empty() {
        result.push('#');
        result.push_str(&format_fragment(&node.display_name));
    }
    Ok(result)
}
