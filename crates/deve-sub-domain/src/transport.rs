//! Transport, multiplex, obfuscation, congestion, and UDP capability types.
//!
//! These are cross-protocol fields lifted to the canonical [`crate::Node`]
//! level. Protocol-specific fields that overlap (e.g. Hysteria2 `obfs`) are
//! projected onto these shared types during parsing. See ADR-0003 and
//! `docs/plan/05-protocol-engine.md`.

use serde::{Deserialize, Serialize};

/// Transport layer kind for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    /// Raw TCP.
    Tcp,
    /// KCP (reliable UDP).
    Kcp,
    /// WebSocket.
    Ws,
    /// HTTP/2.
    H2,
    /// QUIC.
    Quic,
    /// gRPC.
    Grpc,
    /// HTTP Upgrade.
    HttpUpgrade,
    /// XTLS (Vision).
    Xtls,
}

/// Transport configuration. Lifted to the node level so emitters share one
/// shape regardless of protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transport {
    /// Transport kind (TCP, WS, gRPC, etc.).
    pub kind: TransportKind,
    /// Path component for WS, gRPC, or HTTP-Upgrade transports.
    pub path: Option<String>,
    /// HTTP `Host` header for WS/gRPC/H2 transports. This is distinct from
    /// [`crate::Endpoint::host`] — the endpoint host is the connection target,
    /// while this is the Host header sent over the wire.
    pub host: Option<String>,
}

/// UDP capability, three-state per field. `None` means the source did not
/// state a value; the system must not silently fill `Some(true)` for
/// compatibility. See ADR-0005 for the three-state discipline.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UdpCapability {
    /// Whether UDP relay is supported.
    pub supported: Option<bool>,
    /// Whether XUDP (extended UDP) is supported.
    pub xudp: Option<bool>,
}

/// Multiplex configuration (e.g. smux, yamux, h2mux).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiplexConfig {
    /// Multiplex protocol name (e.g. `smux`, `yamux`).
    pub protocol: String,
    /// Maximum number of concurrent connections, if specified.
    pub max_connections: Option<u32>,
}

/// Obfuscation configuration. Hysteria2 `salamander` obfs projects here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obfuscation {
    /// Obfuscation kind (e.g. `salamander`).
    pub kind: String,
    /// Obfuscation password, if required by the kind.
    pub password: Option<String>,
}

/// Congestion controller kind. `Other` carries controllers not yet typed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CongestionController {
    /// BBR (Bottleneck Bandwidth and RTT).
    Bbr,
    /// CUBIC.
    Cubic,
    /// New Reno.
    NewReno,
    /// Controller not yet typed; carries the raw name.
    Other(String),
}

/// Congestion control configuration. Hysteria2 `up`/`down` and TUIC
/// `congestion-controller` project here. Bandwidths are stored in bits per
/// second; emitters convert per target format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CongestionConfig {
    /// Congestion controller algorithm.
    pub controller: CongestionController,
    /// Upload bandwidth in bits per second.
    pub up_bps: Option<u64>,
    /// Download bandwidth in bits per second.
    pub down_bps: Option<u64>,
}
