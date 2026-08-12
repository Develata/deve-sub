//! Typed configuration payloads for the seven P0 protocols and M9 additional
//! protocols (WireGuard, AnyTLS, Snell, ShadowTLS).
//!
//! Fields already lifted to the canonical [`crate::Node`] level (endpoint,
//! authentication, transport, TLS, UDP capability, obfuscation, congestion)
//! are not duplicated here; only protocol-specific fields that have no shared
//! home live in these structs. See ADR-0003 and
//! `docs/plan/05-protocol-engine.md`.

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

/// WireGuard configuration (M9).
///
/// `server`/`port` (the peer endpoint) is carried by [`crate::Endpoint`].
/// WireGuard has **no TLS layer** — it uses Noise IK handshake (X25519 +
/// ChaCha20-Poly1305), so [`crate::TlsConfig`] must be `None`.
///
/// The `private_key` is the local interface key; each peer carries its
/// `public_key`. The `address` list holds local tunnel interface CIDRs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireGuardConfig {
    /// Local interface private key (base64).
    pub private_key: String,
    /// Local tunnel interface addresses as CIDR strings (e.g. `10.0.0.1/32`).
    pub address: Vec<String>,
    /// Peer list. Usually a single peer for proxy use cases.
    pub peers: Vec<WireGuardPeer>,
    /// MTU. Defaults differ by client (mihomo 1420, sing-box 1408).
    pub mtu: Option<u32>,
    /// Worker count (mihomo `workers` field only).
    pub workers: Option<u32>,
    /// DNS resolver addresses (mihomo `dns` field only).
    pub dns: Vec<String>,
}

/// WireGuard peer configuration.
///
/// The peer's `server`/`port` is the node [`crate::Endpoint`]; this struct
/// carries only peer-specific fields not already on the node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireGuardPeer {
    /// Peer public key (base64).
    pub public_key: String,
    /// Pre-shared key (base64), if configured.
    pub pre_shared_key: Option<String>,
    /// Allowed IP CIDRs for this peer.
    pub allowed_ips: Vec<String>,
    /// Reserved bytes (mihomo/sing-box specific, for obfuscation).
    pub reserved: Option<[u8; 3]>,
    /// Persistent keepalive interval.
    pub persistent_keepalive: Option<time::Duration>,
}

/// AnyTLS configuration (M9).
///
/// `password` is carried by [`crate::Authentication::Password`]; TLS fields
/// (sni, alpn, skip_cert_verify, client_fingerprint) by [`crate::TlsConfig`].
/// AnyTLS **always requires TLS** — `node.tls` must be `Some` with
/// `enabled: true`.
///
/// The idle-session tuning fields are sing-box and mihomo extensions for
/// multiplexed session pool management; both clients use the same JSON/YAML
/// key names. `client_metadata` carries the AnyTLS protocol client hello
/// metadata string.
///
/// Nested mihomo obfuscation (`shadow-tls-opts`, `restls-opts`, `jls-opts`)
/// is projected via [`crate::Obfuscation`] or `Node.extras` in a later slice;
/// this struct does not model it yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnyTlsConfig {
    /// Idle session pool check interval (sing-box/mihomo extension).
    pub idle_session_check_interval: Option<time::Duration>,
    /// Idle session timeout before close (sing-box/mihomo extension).
    pub idle_session_timeout: Option<time::Duration>,
    /// Minimum idle sessions to keep open (sing-box/mihomo extension).
    pub min_idle_session: Option<u32>,
    /// AnyTLS client hello metadata string (sing-box/mihomo extension).
    pub client_metadata: Option<String>,
}

/// Snell configuration (M9).
///
/// `psk` (pre-shared key) is carried by [`crate::Authentication::Password`];
/// `server`/`port` by [`crate::Endpoint`]. Snell has **no TLS by default** —
/// TLS only when `obfs.mode` is `Tls`; otherwise [`crate::TlsConfig`] is
/// `None`.
///
/// Version compatibility: mihomo supports V1–V5; sing-box supports V4 and V6
/// only. Emitters must exclude incompatible versions with report (constraint
/// #7). V6 carries an additional `v6_mode` (sing-box only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnellConfig {
    /// Snell protocol version.
    pub version: SnellVersion,
    /// Connection reuse flag (mihomo v4/v5 only).
    pub reuse: Option<bool>,
    /// Obfuscation options (mihomo `obfs-opts`).
    pub obfs: Option<SnellObfs>,
    /// V6 mode (sing-box only: `default`, `unshaped`, `unsafe-raw`).
    pub v6_mode: Option<SnellV6Mode>,
}

/// Snell protocol version.
///
/// mihomo supports V1–V5; sing-box supports V4 and V6 only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SnellVersion {
    /// Snell v1.
    V1,
    /// Snell v2.
    V2,
    /// Snell v3.
    V3,
    /// Snell v4.
    V4,
    /// Snell v5.
    V5,
    /// Snell v6 (sing-box only; carries mode semantics).
    V6,
}

impl SnellVersion {
    /// Returns the numeric version value (1–6).
    #[must_use]
    pub fn as_u32(self) -> u32 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
            Self::V3 => 3,
            Self::V4 => 4,
            Self::V5 => 5,
            Self::V6 => 6,
        }
    }

    /// Parses a numeric version string into a [`SnellVersion`].
    ///
    /// # Errors
    /// Returns `None` if the value is not 1–6.
    #[must_use]
    pub fn from_u32(n: u32) -> Option<Self> {
        match n {
            1 => Some(Self::V1),
            2 => Some(Self::V2),
            3 => Some(Self::V3),
            4 => Some(Self::V4),
            5 => Some(Self::V5),
            6 => Some(Self::V6),
            _ => None,
        }
    }
}

/// Snell V6 mode (sing-box only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SnellV6Mode {
    /// Default V6 mode.
    Default,
    /// Unshaped V6 mode.
    Unshaped,
    /// Unsafe-raw V6 mode.
    UnsafeRaw,
}

/// Snell obfuscation options (mihomo `obfs-opts`).
///
/// `mode` selects the obfuscation strategy. `host` and `password` apply to
/// TLS/HTTP/ShadowTLS/Restls/Jls modes. `version` is the ShadowTLS sub-version
/// when `mode = ShadowTls`. `alpn` applies to TLS-shaped modes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnellObfs {
    /// Obfuscation mode.
    pub mode: SnellObfsMode,
    /// Obfuscation host (e.g. `bing.com`).
    pub host: Option<String>,
    /// Obfuscation password (ShadowTLS/Restls/Jls).
    pub password: Option<String>,
    /// ShadowTLS sub-version (when `mode = ShadowTls`).
    pub version: Option<u32>,
    /// ALPN list (TLS-shaped modes).
    pub alpn: Vec<String>,
}

/// Snell obfuscation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SnellObfsMode {
    /// Simple TLS obfuscation.
    Tls,
    /// HTTP obfuscation.
    Http,
    /// ShadowTLS obfuscation (nested; see M9 Slice 4).
    ShadowTls,
    /// Restls obfuscation.
    Restls,
    /// JLS obfuscation.
    Jls,
}

/// ShadowTLS configuration (M9).
///
/// ShadowTLS is a TLS-camouflage wrapper: it performs a real TLS handshake
/// against a camouflage server (SNI), then tunnels an inner protocol inside.
/// The camouflage TLS fields (sni, alpn, skip_cert_verify, fingerprint) live
/// on [`crate::TlsConfig`] attached to the outer `Node`.
///
/// `inner_protocol` + `inner_config` carry the wrapped protocol. When
/// emitting to sing-box, the ShadowTLS outbound is standalone and the inner
/// protocol outbound chains via `detour`. When emitting to mihomo,
/// ShadowTLS is projected as an obfuscation layer under the inner protocol
/// type (`shadow-tls-opts` for vless/trojan/vmess/anytls, `plugin:
/// shadow-tls` for ss, `obfs-opts.mode: shadow-tls` for snell). Xray does
/// not support ShadowTLS — excluded with report (constraint #7).
///
/// `password` is required for V2/V3 and `None` for V1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowTlsConfig {
    /// ShadowTLS protocol version.
    pub version: ShadowTlsVersion,
    /// ShadowTLS password (required for V2/V3, `None` for V1).
    pub password: Option<String>,
    /// The protocol wrapped inside ShadowTLS.
    pub inner_protocol: crate::ProtocolKind,
    /// Typed config of the inner protocol.
    pub inner_config: Box<crate::ProtocolConfig>,
}

/// ShadowTLS protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShadowTlsVersion {
    /// ShadowTLS v1 (no password).
    V1,
    /// ShadowTLS v2 (password required).
    V2,
    /// ShadowTLS v3 (password required).
    V3,
}

impl ShadowTlsVersion {
    /// Returns the numeric version value (1–3).
    #[must_use]
    pub fn as_u32(self) -> u32 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
            Self::V3 => 3,
        }
    }

    /// Parses a numeric version into a [`ShadowTlsVersion`].
    ///
    /// # Errors
    /// Returns `None` if the value is not 1–3.
    #[must_use]
    pub fn from_u32(n: u32) -> Option<Self> {
        match n {
            1 => Some(Self::V1),
            2 => Some(Self::V2),
            3 => Some(Self::V3),
            _ => None,
        }
    }
}
