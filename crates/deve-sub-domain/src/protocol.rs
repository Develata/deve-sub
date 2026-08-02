//! Protocol identification and configuration envelope.
//!
//! `ProtocolKind` identifies the wire-level protocol (fifteen typed variants
//! plus `Unknown(String)`). `ProtocolConfig` carries typed payloads for the
//! seven P0 protocols only; non-P0 or unrecognized nodes fall back to the
//! `Unsupported` variant, which preserves raw data without claiming support.
//! See ADR-0003.

use serde::{Deserialize, Serialize};

use crate::protocol_config::{
    Hysteria2Config, NaiveProxyConfig, ShadowsocksConfig, TrojanConfig, TuicV5Config, VMessConfig,
    VlessRealityConfig,
};

/// Wire-level proxy protocol kind.
///
/// Fifteen typed variants match the protocol enumeration in
/// `docs/plan/05-protocol-engine.md`. `Unknown(String)` retains protocols not
/// yet typed so they are preserved rather than silently dropped (constraint
/// #7).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ProtocolKind {
    Vless,
    VMess,
    Trojan,
    Shadowsocks,
    Hysteria2,
    TuicV5,
    NaiveProxy,
    Socks5,
    Http,
    HysteriaV1,
    AnyTls,
    Snell,
    WireGuard,
    ShadowTls,
    Ssh,
    Unknown(String),
}

/// Typed configuration payload for a node.
///
/// The seven P0 variants carry fully typed config. The `Unsupported` variant
/// wraps [`UnsupportedNode`] for non-P0 or unrecognized protocols: it is
/// stored in the node pool but excluded from emitters and is not claimed as
/// supported. See ADR-0003.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolConfig {
    VlessReality(VlessRealityConfig),
    Hysteria2(Hysteria2Config),
    TuicV5(TuicV5Config),
    NaiveProxy(NaiveProxyConfig),
    Shadowsocks(ShadowsocksConfig),
    VMess(VMessConfig),
    Trojan(TrojanConfig),
    /// Fallback for non-P0 or unrecognized protocols. Preserves raw data;
    /// emitters must skip it. See ADR-0003.
    Unsupported(UnsupportedNode),
}

/// Preserved raw data for a node whose protocol is not P0 or not recognized.
///
/// Stored in the unified node pool so non-P0 nodes are never silently dropped
/// (constraint #7), but excluded from all emitters so no false support claim is
/// made (constraint #3). See ADR-0003.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnsupportedNode {
    /// Original raw payload, preserved verbatim for future typed support.
    pub raw: serde_json::Value,
    /// Input format the raw payload was read from, if known.
    pub raw_format: Option<String>,
    /// Human-readable reason this node is unsupported.
    pub reason: String,
}
