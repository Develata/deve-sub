//! The canonical [`Node`] aggregate and its supporting types.
//!
//! See ADR-0003 for the canonical node model decision and
//! `docs/plan/05-protocol-engine.md` for the full blueprint. This module
//! depends only on [`deve_sub_kernel`] and sibling domain modules.

use std::collections::BTreeMap;

use deve_sub_kernel::{NodeId, TagId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::endpoint::Endpoint;
use crate::protocol::{ProtocolConfig, ProtocolKind};
use crate::tls::TlsConfig;
use crate::transport::{CongestionConfig, MultiplexConfig, Obfuscation, Transport, UdpCapability};

/// The canonical node model: the single normalized representation of a proxy
/// node, independent of input format and output target. All parsers produce
/// it; all emitters consume it. See ADR-0003.
///
/// WHY: `protocol` and `config` are independent public fields, so inconsistent
/// pairings (e.g. `ProtocolKind::Trojan` + `ProtocolConfig::VMess(...)`) are
/// representable. The kind↔config invariant is upheld by parsers and emitters
/// (M3), not by the type system; see [`ProtocolConfig`] for the VLESS Reality
/// scoping rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// Server-monotonic unique identifier.
    pub id: NodeId,
    /// Human-readable label shown in the UI and subscription output.
    pub display_name: String,
    /// Wire-level protocol kind. See [`ProtocolKind`] and ADR-0003.
    pub protocol: ProtocolKind,
    /// Typed protocol configuration. The variant must be consistent with
    /// `protocol`; see the struct-level WHY note on the pairing invariant.
    pub config: ProtocolConfig,
    /// Network endpoint (host + port) the proxy connects to.
    pub endpoint: Endpoint,
    /// Authentication credentials, lifted to the node level. The variant
    /// depends on [`ProtocolKind`].
    pub authentication: Authentication,
    /// Transport-layer config (WS, gRPC, H2, etc.). `None` means the protocol
    /// default (typically raw TCP/UDP).
    pub transport: Option<Transport>,
    /// TLS settings. `None` means the protocol does not use TLS (e.g.
    /// Shadowsocks, plain HTTP). When present, [`TlsConfig::enabled`]
    /// distinguishes explicit TLS on/off.
    pub tls: Option<TlsConfig>,
    /// UDP relay capability, three-state per field. Defaults to `None`/`None`
    /// when the source does not state a value. See ADR-0005.
    pub udp: UdpCapability,
    /// Multiplex configuration (smux, yamux, etc.). `None` means no mux.
    pub multiplex: Option<MultiplexConfig>,
    /// Obfuscation configuration (e.g. Hysteria2 salamander). `None` means no
    /// obfuscation.
    pub obfuscation: Option<Obfuscation>,
    /// Congestion control configuration. `None` means protocol default.
    pub congestion: Option<CongestionConfig>,
    /// Chain target: route traffic through another node first. `None` means
    /// direct connection.
    pub chain: Option<ChainTarget>,
    /// Provenance of the node (source label, raw URI, import timestamp).
    pub source: NodeSource,
    /// User-assigned tags for grouping and filtering.
    pub tags: Vec<TagId>,
    /// Region assignment (auto-detected or manual override).
    pub region: RegionAssignment,
    /// Protocol-specific fields that have no typed home. Forward-compatible
    /// escape hatch; emitters must round-trip unknown keys unchanged.
    pub extras: BTreeMap<String, serde_json::Value>,
}

/// Authentication credentials, lifted to the node level. The variant used
/// depends on [`ProtocolKind`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Authentication {
    /// VLESS, VMess, TUIC v5 uuid.
    Uuid { uuid: String },
    /// Hysteria2, Trojan password.
    Password { password: String },
    /// NaiveProxy username+password.
    UserPassword { username: String, password: String },
    /// Shadowsocks method+password.
    Shadowsocks { method: String, password: String },
    /// TUIC v5 uuid+password.
    UuidPassword { uuid: String, password: String },
    /// No authentication (e.g. unauthed Socks5/HTTP).
    None,
}

/// Chain target: route traffic through another node first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainTarget {
    /// Entry node of the chain.
    pub entry_node_id: NodeId,
}

/// Provenance of a node within the unified pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSource {
    /// Human-readable label of the subscription source. A typed `SourceId`
    /// foreign key replaces this when the Source aggregate lands in M2.
    pub source_label: String,
    /// Original share URI or raw fragment, if the node came from a URI list.
    /// Sensitive: typically embeds credentials; the persistence adapter must
    /// include this in the encryption set (XChaCha20-Poly1305).
    pub raw_uri: Option<String>,
    /// Import timestamp, distinct from the ULID's embedded time.
    pub imported_at: Timestamp,
}

/// Region assignment for a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionAssignment {
    /// How the region was assigned.
    pub method: RegionMethod,
    /// ISO region code or free-form label. `None` when auto-detection has not
    /// run yet. `RegionMethod::Manual` implies `Some` — an admin override
    /// always carries a value.
    pub value: Option<String>,
}

/// How a node's region was assigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionMethod {
    /// GeoIP-derived.
    Auto,
    /// Admin-authored override. Remote updates must not overwrite this.
    Manual,
}
