//! Typed configuration payloads for the seven P0 protocols.
//!
//! Only P0 protocols have typed config in Phase 1. Fields already lifted to
//! the canonical [`crate::Node`] level (endpoint, authentication, transport,
//! TLS, UDP capability, obfuscation, congestion) are not duplicated here; only
//! protocol-specific fields that have no shared home live in these structs.
//! See ADR-0003 and `docs/plan/05-protocol-engine.md`.

use serde::{Deserialize, Serialize};

/// VLESS Reality configuration.
///
/// `uuid` is carried by [`crate::Authentication::Uuid`]; `server`/`port` by
/// [`crate::Endpoint`]; `network` by [`crate::Transport`]; `sni`, `fp`,
/// `allowInsecure`, and Reality `pbk`/`sid`/`spx` by [`crate::TlsConfig`];
/// `udp`/`xudp` by [`crate::UdpCapability`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VlessRealityConfig {
    /// `encryption` query parameter, conventionally `none`.
    pub encryption: Option<String>,
    /// `flow` query parameter, e.g. `xtls-rprx-vision`. Output profiles
    /// without Vision support must exclude the node and report it.
    pub flow: Option<String>,
    /// `packetEncoding` query parameter.
    pub packet_encoding: Option<String>,
}

/// Hysteria2 configuration.
///
/// `password`/`auth` is carried by [`crate::Authentication::Password`]; TLS
/// fields by [`crate::TlsConfig`]; `obfs`/`obfs-password` by
/// [`crate::Obfuscation`]; `up`/`down` by [`crate::CongestionConfig`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hysteria2Config {
    /// Port hopping range string, e.g. `20000-40000`.
    pub ports: Option<String>,
    /// Hop interval. Emitters convert per target and must never mix seconds
    /// and milliseconds.
    pub hop_interval: Option<time::Duration>,
    /// `fast-open` query parameter.
    pub fast_open: Option<bool>,
    /// `lazy` query parameter.
    pub lazy: Option<bool>,
}

/// TUIC v5 configuration.
///
/// `uuid`/`password` is carried by [`crate::Authentication::UuidPassword`];
/// TLS fields by [`crate::TlsConfig`]; `congestion-controller` by
/// [`crate::CongestionConfig`]. The `token` auth mode listed in plan/05
/// §"TUIC v5" is not yet modeled — it lands with the M3 parsing layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuicV5Config {
    /// `udp-relay-mode` query parameter.
    pub udp_relay_mode: Option<UdpRelayMode>,
    /// `zero-rtt-handshake` query parameter.
    pub zero_rtt_handshake: Option<bool>,
    /// Heartbeat interval. Emitters convert per target and must never mix
    /// seconds and milliseconds.
    pub heartbeat: Option<time::Duration>,
    /// `disable-sni` query parameter.
    pub disable_sni: Option<bool>,
}

/// TUIC v5 UDP relay mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UdpRelayMode {
    /// Native UDP relay.
    Native,
    /// QUIC-based UDP relay.
    Quic,
}

/// NaiveProxy configuration.
///
/// `username`/`password` is carried by
/// [`crate::Authentication::UserPassword`]; TLS fields by [`crate::TlsConfig`].
/// Naive must not be downgraded to a plain HTTP node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NaiveProxyConfig {
    /// Enable QUIC transport.
    pub quic: Option<bool>,
    /// Enable HTTP/2.
    pub http2: Option<bool>,
    /// Enable HTTP/3.
    pub http3: Option<bool>,
}

/// Shadowsocks configuration.
///
/// `password` is carried by [`crate::Authentication::Password`];
/// `server`/`port` by [`crate::Endpoint`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowsocksConfig {
    /// Cipher method (e.g. `aes-256-gcm`, `chacha20-ietf-poly1305`).
    pub method: String,
    /// SIP003 plugin name, if any.
    pub plugin: Option<String>,
    /// SIP003 plugin options string.
    pub plugin_opts: Option<String>,
}

/// VMess configuration.
///
/// `uuid` is carried by [`crate::Authentication::Uuid`]; `server`/`port` by
/// [`crate::Endpoint`]; `network` by [`crate::Transport`]; TLS by
/// [`crate::TlsConfig`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VMessConfig {
    /// `alterId`. Deprecated in modern VMess but preserved for fidelity.
    pub alter_id: Option<u32>,
    /// `security`/encryption, e.g. `auto`, `aes-128-gcm`, `none`.
    pub security: Option<String>,
    /// `packetEncoding` query parameter.
    pub packet_encoding: Option<String>,
}

/// Trojan configuration.
///
/// `password` is carried by [`crate::Authentication::Password`];
/// `server`/`port` by [`crate::Endpoint`]; TLS by [`crate::TlsConfig`];
/// `network` by [`crate::Transport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrojanConfig {
    /// `packetEncoding` query parameter.
    pub packet_encoding: Option<String>,
}
