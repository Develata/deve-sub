//! Transport, multiplex, obfuscation, congestion, and UDP capability types.
//!
//! These are cross-protocol fields lifted to the canonical [`crate::Node`]
//! level. Protocol-specific fields that overlap (e.g. Hysteria2 `obfs`) are
//! projected onto these shared types during parsing. See
//! `docs/plan/05-protocol-engine.md`.

use serde::{Deserialize, Serialize};

/// Transport layer kind for a node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Tcp,
    Kcp,
    Ws,
    H2,
    Quic,
    Grpc,
    HttpUpgrade,
    Xtls,
}

/// Transport configuration. Lifted to the node level so emitters share one
/// shape regardless of protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transport {
    pub kind: TransportKind,
    pub path: Option<String>,
    pub host: Option<String>,
}

/// UDP capability, three-state per field. `None` means the source did not
/// state a value; the system must not silently fill `Some(true)` for
/// compatibility. See ADR-0005 for the three-state discipline.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UdpCapability {
    pub supported: Option<bool>,
    pub xudp: Option<bool>,
}

/// Multiplex configuration (e.g. smux, yamux, h2mux).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiplexConfig {
    pub protocol: String,
    pub max_connections: Option<u32>,
}

/// Obfuscation configuration. Hysteria2 `salamander` obfs projects here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obfuscation {
    pub kind: String,
    pub password: Option<String>,
}

/// Congestion controller kind. `Other` carries controllers not yet typed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CongestionController {
    Bbr,
    Cubic,
    NewReno,
    Other(String),
}

/// Congestion control configuration. Hysteria2 `up`/`down` and TUIC
/// `congestion-controller` project here. Bandwidths are stored in bits per
/// second; emitters convert per target format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CongestionConfig {
    pub controller: CongestionController,
    pub up_bps: Option<u64>,
    pub down_bps: Option<u64>,
}
