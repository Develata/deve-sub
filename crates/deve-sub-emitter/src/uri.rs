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
    match node.protocol {
        ProtocolKind::Vless => crate::vless_reality::emit(node),
        ProtocolKind::Trojan => crate::trojan::emit(node),
        ProtocolKind::Hysteria2 => crate::hysteria2::emit(node),
        ProtocolKind::TuicV5 => crate::tuic_v5::emit(node),
        ProtocolKind::NaiveProxy => crate::naive::emit(node),
        ProtocolKind::Shadowsocks => crate::shadowsocks::emit(node),
        ProtocolKind::VMess => crate::vmess::emit(node),
        ref other => Err(EmitError::NoEmitter(other.to_string())),
    }
}
