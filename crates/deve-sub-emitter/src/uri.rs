//! URI emission dispatcher.
//!
//! The top-level [`emit_uri`] function dispatches on `node.protocol` to the
//! appropriate protocol emitter.

use deve_sub_domain::{Node, ProtocolKind};

use crate::error::EmitError;

/// Emit a canonical [`Node`] as a share URI string.
///
/// Dispatches on `node.protocol`. Returns [`EmitError::NoEmitter`] for
/// protocols without a URI emitter (e.g. `Unsupported` nodes).
///
/// # Errors
/// Returns [`EmitError`] if the protocol has no emitter or a required field
/// is missing.
pub fn emit_uri(node: &Node) -> Result<String, EmitError> {
    if !matches!(node.protocol, ProtocolKind::Socks5 | ProtocolKind::Http)
        && let Some(tls) = &node.tls
        && !tls.certificate_pins.is_empty()
        && (!tls.enabled
            || tls.reality.is_some()
            || !matches!(
                node.protocol,
                ProtocolKind::Hysteria2 | ProtocolKind::NaiveProxy
            )
            || !tls
                .certificate_pins
                .iter()
                .all(|p| p.as_str().starts_with("pinSHA256:")))
    {
        return Err(EmitError::NoEmitter(
            "certificate pin representation".into(),
        ));
    }
    match node.protocol {
        ProtocolKind::Socks5 | ProtocolKind::Http => crate::forward_proxy::emit(node),
        ProtocolKind::Vless => crate::vless_reality::emit(node),
        ProtocolKind::Trojan => crate::trojan::emit(node),
        ProtocolKind::Hysteria2 => crate::hysteria2::emit(node),
        ProtocolKind::TuicV5 => crate::tuic_v5::emit(node),
        ProtocolKind::NaiveProxy => crate::naive::emit(node),
        ProtocolKind::Shadowsocks => crate::shadowsocks::emit(node),
        ProtocolKind::VMess => crate::vmess::emit(node),
        ProtocolKind::WireGuard => crate::wireguard::emit(node),
        ProtocolKind::AnyTls => crate::anytls::emit(node),
        ProtocolKind::Snell => crate::snell::emit(node),
        ProtocolKind::ShadowTls => crate::shadowtls::emit(node),
        ref other => Err(EmitError::NoEmitter(other.to_string())),
    }
}
