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
    /// XHTTP (Xray's split-HTTP transport, also known as `splithttp`).
    /// Supported by Xray and Mihomo; not supported by sing-box.
    Xhttp,
}

/// XHTTP transport mode. Controls how the inner protocol stream is split
/// across HTTP requests. See Xray `xhttp` / `splithttp` documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum XhttpMode {
    /// Auto-select mode based on the protocol (Xray default).
    #[default]
    Auto,
    /// Stream-one: a single long-lived HTTP request carries the full stream.
    StreamOne,
    /// Stream-up: upstream data is split across multiple POST requests,
    /// downstream is a single long-poll response.
    StreamUp,
    /// Packet-up: each packet is a separate POST request, downstream is
    /// a single long-poll response.
    PacketUp,
}

impl XhttpMode {
    /// Parse a mode from its URI/query string representation.
    ///
    /// Accepts `auto`, `stream-one`, `stream-up`, `packet-up`. Returns
    /// `None` for unrecognized values (the caller decides whether to
    /// treat this as an error or fall back to `Auto`).
    #[must_use]
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "stream-one" => Some(Self::StreamOne),
            "stream-up" => Some(Self::StreamUp),
            "packet-up" => Some(Self::PacketUp),
            _ => None,
        }
    }

    /// Return the string used in URI query parameters and JSON config.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::StreamOne => "stream-one",
            Self::StreamUp => "stream-up",
            Self::PacketUp => "packet-up",
        }
    }
}

impl std::fmt::Display for XhttpMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Display for TransportKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tcp => write!(f, "tcp"),
            Self::Kcp => write!(f, "kcp"),
            Self::Ws => write!(f, "ws"),
            Self::H2 => write!(f, "h2"),
            Self::Quic => write!(f, "quic"),
            Self::Grpc => write!(f, "grpc"),
            Self::HttpUpgrade => write!(f, "httpupgrade"),
            Self::Xtls => write!(f, "xtls"),
            Self::Xhttp => write!(f, "xhttp"),
        }
    }
}

/// Transport configuration. Lifted to the node level so emitters share one
/// shape regardless of protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transport {
    /// Transport kind (TCP, WS, gRPC, XHTTP, etc.).
    pub kind: TransportKind,
    /// Path component for WS, gRPC, HTTP-Upgrade, or XHTTP transports.
    pub path: Option<String>,
    /// HTTP `Host` header for WS/gRPC/H2/XHTTP transports. This is distinct
    /// from [`crate::Endpoint::host`] — the endpoint host is the connection
    /// target, while this is the Host header sent over the wire.
    pub host: Option<String>,
    /// XHTTP mode (only meaningful when `kind == TransportKind::Xhttp`).
    /// `None` means the source did not specify; emitters default to `Auto`.
    pub xhttp_mode: Option<XhttpMode>,
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
