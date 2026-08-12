//! Protocol identification and configuration envelope.
//!
//! `ProtocolKind` identifies the wire-level protocol (fifteen typed variants
//! plus `Unknown(String)`). `ProtocolConfig` carries typed payloads for the
//! seven P0 protocols only; non-P0 or unrecognized nodes fall back to the
//! `Unsupported` variant, which preserves raw data without claiming support.
//! See ADR-0003.

use serde::{Deserialize, Serialize};

use crate::protocol_config::{
    AnyTlsConfig, Hysteria2Config, NaiveProxyConfig, ShadowsocksConfig, SnellConfig, TrojanConfig,
    TuicV5Config, VMessConfig, VlessRealityConfig, WireGuardConfig,
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
    /// VLESS. P0 scopes to Reality only; see [`ProtocolConfig::VlessReality`].
    Vless,
    /// VMess (V2Ray encrypted protocol).
    VMess,
    /// Trojan.
    Trojan,
    /// Shadowsocks.
    Shadowsocks,
    /// Hysteria2.
    Hysteria2,
    /// TUIC v5.
    TuicV5,
    /// NaiveProxy.
    NaiveProxy,
    // --- Non-P0: stored as ProtocolConfig::Unsupported until typed config lands. ---
    /// SOCKS5 proxy. Non-P0.
    Socks5,
    /// HTTP/HTTPS proxy. Non-P0.
    Http,
    /// Hysteria v1. Non-P0.
    HysteriaV1,
    /// AnyTLS. Typed in M9; see [`ProtocolConfig::AnyTls`].
    AnyTls,
    /// Snell. Typed in M9; see [`ProtocolConfig::Snell`].
    Snell,
    /// WireGuard. Typed in M9; see [`ProtocolConfig::WireGuard`].
    WireGuard,
    /// ShadowTLS. Non-P0.
    ShadowTls,
    /// SSH tunnel. Non-P0.
    Ssh,
    /// Protocol not yet typed; carries the raw name so the node is preserved
    /// rather than silently dropped (constraint #7).
    Unknown(String),
}

impl std::fmt::Display for ProtocolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vless => write!(f, "VLESS"),
            Self::VMess => write!(f, "VMess"),
            Self::Trojan => write!(f, "Trojan"),
            Self::Shadowsocks => write!(f, "Shadowsocks"),
            Self::Hysteria2 => write!(f, "Hysteria2"),
            Self::TuicV5 => write!(f, "TUIC v5"),
            Self::NaiveProxy => write!(f, "NaiveProxy"),
            Self::Socks5 => write!(f, "SOCKS5"),
            Self::Http => write!(f, "HTTP"),
            Self::HysteriaV1 => write!(f, "Hysteria v1"),
            Self::AnyTls => write!(f, "AnyTLS"),
            Self::Snell => write!(f, "Snell"),
            Self::WireGuard => write!(f, "WireGuard"),
            Self::ShadowTls => write!(f, "ShadowTLS"),
            Self::Ssh => write!(f, "SSH"),
            Self::Unknown(name) => write!(f, "Unknown({name})"),
        }
    }
}

impl ProtocolKind {
    /// Stable, machine-friendly key used for source filter matching (SRC-010).
    ///
    /// WHY: [`Display`](std::fmt::Display) output is tuned for human-readable
    /// UI labels (e.g. `"TUIC v5"`, `"Hysteria v1"`). Source filter rules need
    /// a key whose match semantics do not shift when the Display label is
    /// reworded for aesthetics. This method returns a lowercase, hyphenated
    /// identifier that is independent of Display and stable across UI changes.
    /// Comparison in the filter is case-insensitive, so `"VLESS"` and `"vless"`
    /// both match.
    #[must_use]
    pub fn as_filter_key(&self) -> &str {
        match self {
            Self::Vless => "vless",
            Self::VMess => "vmess",
            Self::Trojan => "trojan",
            Self::Shadowsocks => "shadowsocks",
            Self::Hysteria2 => "hysteria2",
            Self::TuicV5 => "tuic-v5",
            Self::NaiveProxy => "naiveproxy",
            Self::Socks5 => "socks5",
            Self::Http => "http",
            Self::HysteriaV1 => "hysteria-v1",
            Self::AnyTls => "anytls",
            Self::Snell => "snell",
            Self::WireGuard => "wireguard",
            Self::ShadowTls => "shadowtls",
            Self::Ssh => "ssh",
            Self::Unknown(name) => name.as_str(),
        }
    }
}

/// Typed configuration payload for a node.
///
/// The seven P0 variants carry fully typed config. The `Unsupported` variant
/// wraps [`UnsupportedNode`] for non-P0 or unrecognized protocols: it is
/// stored in the node pool but excluded from emitters and is not claimed as
/// supported. See ADR-0003.
///
/// Note on VLESS: P0 scopes VLESS support to Reality only, so
/// [`ProtocolKind::Vless`](crate::ProtocolKind::Vless) pairs with
/// [`ProtocolConfig::VlessReality`]. A non-Reality VLESS node is valid as
/// `ProtocolKind::Vless` + `ProtocolConfig::Unsupported`; emitters matching
/// on `ProtocolKind::Vless` must not assume `VlessRealityConfig`.
///
/// `#[non_exhaustive]` signals that new typed variants will be added as
/// non-P0 protocols gain typed config (HysteriaV1, AnyTls, etc.); downstream
/// match arms must include a `_` wildcard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProtocolConfig {
    /// VLESS Reality configuration. P0 scopes VLESS to Reality only.
    VlessReality(VlessRealityConfig),
    /// Hysteria2 configuration.
    Hysteria2(Hysteria2Config),
    /// TUIC v5 configuration.
    TuicV5(TuicV5Config),
    /// NaiveProxy configuration.
    NaiveProxy(NaiveProxyConfig),
    /// Shadowsocks configuration.
    Shadowsocks(ShadowsocksConfig),
    /// VMess configuration.
    VMess(VMessConfig),
    /// Trojan configuration.
    Trojan(TrojanConfig),
    /// WireGuard configuration (M9).
    WireGuard(WireGuardConfig),
    /// AnyTLS configuration (M9).
    AnyTls(AnyTlsConfig),
    /// Snell configuration (M9).
    Snell(SnellConfig),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_kind_serde_names() {
        // Lock the PascalCase serialized names — especially the mixed-case
        // variants (VMess, HysteriaV1, TuicV5, AnyTls, ShadowTls) that a
        // serde/heck version bump could silently alter.
        let cases = [
            ("Vless", ProtocolKind::Vless),
            ("VMess", ProtocolKind::VMess),
            ("Trojan", ProtocolKind::Trojan),
            ("Shadowsocks", ProtocolKind::Shadowsocks),
            ("Hysteria2", ProtocolKind::Hysteria2),
            ("TuicV5", ProtocolKind::TuicV5),
            ("NaiveProxy", ProtocolKind::NaiveProxy),
            ("Socks5", ProtocolKind::Socks5),
            ("Http", ProtocolKind::Http),
            ("HysteriaV1", ProtocolKind::HysteriaV1),
            ("AnyTls", ProtocolKind::AnyTls),
            ("Snell", ProtocolKind::Snell),
            ("WireGuard", ProtocolKind::WireGuard),
            ("ShadowTls", ProtocolKind::ShadowTls),
            ("Ssh", ProtocolKind::Ssh),
        ];
        for (expected, kind) in cases {
            let json = serde_json::to_string(&kind).expect("serialize");
            assert_eq!(json, format!("\"{expected}\""));
            let recovered: ProtocolKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(recovered, kind);
        }
    }

    #[test]
    fn protocol_kind_unknown_serde() {
        let kind = ProtocolKind::Unknown("FutureProto".to_owned());
        let json = serde_json::to_string(&kind).expect("serialize");
        assert_eq!(json, "{\"Unknown\":\"FutureProto\"}");
        let recovered: ProtocolKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(recovered, kind);
    }

    #[test]
    fn as_filter_key_stable() {
        let cases = [
            (ProtocolKind::Vless, "vless"),
            (ProtocolKind::VMess, "vmess"),
            (ProtocolKind::Trojan, "trojan"),
            (ProtocolKind::Shadowsocks, "shadowsocks"),
            (ProtocolKind::Hysteria2, "hysteria2"),
            (ProtocolKind::TuicV5, "tuic-v5"),
            (ProtocolKind::NaiveProxy, "naiveproxy"),
            (ProtocolKind::Socks5, "socks5"),
            (ProtocolKind::Http, "http"),
            (ProtocolKind::HysteriaV1, "hysteria-v1"),
            (ProtocolKind::AnyTls, "anytls"),
            (ProtocolKind::Snell, "snell"),
            (ProtocolKind::WireGuard, "wireguard"),
            (ProtocolKind::ShadowTls, "shadowtls"),
            (ProtocolKind::Ssh, "ssh"),
        ];
        for (kind, expected) in cases {
            assert_eq!(
                kind.as_filter_key(),
                expected,
                "filter key for {kind:?} must stay stable"
            );
        }
        assert_eq!(
            ProtocolKind::Unknown("CustomProto".to_owned()).as_filter_key(),
            "CustomProto",
            "Unknown passes the raw name through"
        );
    }
}
