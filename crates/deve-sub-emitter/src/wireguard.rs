//! WireGuard URI emitter.
//!
//! Serializes a canonical [`deve_sub_domain::Node`] with
//! `ProtocolKind::WireGuard` + `ProtocolConfig::WireGuard` back to a
//! `wireguard://` share URI.
//!
//! ## Query parameter order
//!
//! `publickey`, `address`, `presharedkey`, `reserved`, `mtu`, `keepalive`.

use deve_sub_domain::{Authentication, Node, ProtocolConfig, WireGuardConfig};

use crate::common::{format_fragment, format_query};
use crate::error::EmitError;

/// Emit a WireGuard [`Node`] as a `wireguard://` share URI.
pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    if !matches!(node.authentication, Authentication::None) {
        return Err(EmitError::MissingField("wireguard uses no authentication"));
    }

    let WireGuardConfig {
        private_key,
        address,
        peers,
        mtu,
        workers: _,
        dns: _,
    } = match &node.config {
        ProtocolConfig::WireGuard(c) => c,
        _ => return Err(EmitError::NoEmitter("non-WireGuard config".to_owned())),
    };

    let peer = peers
        .first()
        .ok_or(EmitError::MissingField("wireguard peer"))?;

    let mut params: Vec<(String, String)> = Vec::new();
    params.push(("publickey".to_owned(), peer.public_key.clone()));

    if !address.is_empty() {
        params.push(("address".to_owned(), address.join(",")));
    }

    if let Some(ref psk) = peer.pre_shared_key {
        params.push(("presharedkey".to_owned(), psk.clone()));
    }

    if let Some(reserved) = peer.reserved {
        params.push((
            "reserved".to_owned(),
            format!("{},{},{}", reserved[0], reserved[1], reserved[2]),
        ));
    }

    if let Some(mtu_val) = mtu {
        params.push(("mtu".to_owned(), mtu_val.to_string()));
    }

    if let Some(keepalive) = peer.persistent_keepalive {
        let secs = keepalive.whole_seconds();
        if secs >= 0 {
            params.push(("keepalive".to_owned(), secs.to_string()));
        }
    }

    let query = format_query(&params);

    let mut result = format!(
        "wireguard://{private_key}@{host}:{port}?{query}",
        host = node.endpoint.host.uri_host(),
        port = node.endpoint.port,
    );
    if !node.display_name.is_empty() {
        result.push('#');
        result.push_str(&format_fragment(&node.display_name));
    }
    Ok(result)
}
