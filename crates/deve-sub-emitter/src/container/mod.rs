//! Container-format emission for target profiles.
//!
//! Each emitter maps a slice of canonical [`Node`] values to a
//! format-conforming document for a target client. The current scope
//! (M5 Slice 4) emits proxy/outbound entries only; group, rule, DNS, and
//! TUN assembly lands in Slice 5.
//!
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"Container
//! emitters".

pub mod ir;
pub mod mihomo;
pub mod singbox;
pub mod v2ray;
pub mod xray;

use deve_sub_domain::Node;

use crate::error::EmitError;

pub use ir::{AssembledGroup, AssembledTemplate};

/// Emit a Mihomo (Clash Meta) YAML document with only proxy entries (no
/// proxy-groups, rules, dns, or tun).
///
/// For the full template document (proxies + proxy-groups + rules + dns +
/// tun), use [`emit_mihomo_full`].
pub fn emit_mihomo(nodes: &[Node]) -> Result<String, EmitError> {
    mihomo::emit(nodes)
}

/// Emit a full Mihomo (Clash Meta) YAML document from an assembled template:
/// proxies, proxy-groups, rules, dns, and tun sections.
///
/// Pass groups/rules/dns/tun via [`AssembledTemplate`] so the emitter maps
/// them to the target document instead of dropping them.
pub fn emit_mihomo_full(template: &AssembledTemplate) -> Result<String, EmitError> {
    mihomo::emit_full(template)
}

/// Emit a sing-box JSON document.
pub fn emit_singbox(nodes: &[Node]) -> Result<String, EmitError> {
    singbox::emit(nodes)
}

/// Emit an Xray JSON document.
pub fn emit_xray(nodes: &[Node]) -> Result<String, EmitError> {
    xray::emit(nodes)
}

/// Emit a V2Ray JSON document.
pub fn emit_v2ray(nodes: &[Node]) -> Result<String, EmitError> {
    v2ray::emit(nodes)
}

/// Emit a Shadowrocket subscription (URI list with base64 encoding).
pub fn emit_shadowrocket(nodes: &[Node]) -> Result<String, EmitError> {
    let uris: Result<Vec<String>, EmitError> = nodes.iter().map(crate::uri::emit_uri).collect();
    let joined = uris?.join("\n");
    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(joined))
}
