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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub display_name: String,
    pub protocol: ProtocolKind,
    pub config: ProtocolConfig,
    pub endpoint: Endpoint,
    pub authentication: Authentication,
    pub transport: Option<Transport>,
    pub tls: Option<TlsConfig>,
    pub udp: UdpCapability,
    pub multiplex: Option<MultiplexConfig>,
    pub obfuscation: Option<Obfuscation>,
    pub congestion: Option<CongestionConfig>,
    pub chain: Option<ChainTarget>,
    pub source: NodeSource,
    pub tags: Vec<TagId>,
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
    Uuid {
        uuid: String,
    },
    /// Hysteria2, Trojan password.
    Password {
        password: String,
    },
    /// NaiveProxy username+password.
    UserPassword {
        username: String,
        password: String,
    },
    /// Shadowsocks method+password.
    Shadowsocks {
        method: String,
        password: String,
    },
    /// TUIC v5 uuid+password.
    UuidPassword {
        uuid: String,
        password: String,
    },
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
    pub raw_uri: Option<String>,
    pub imported_at: Timestamp,
}

/// Region assignment for a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionAssignment {
    pub method: RegionMethod,
    /// ISO region code or free-form label. `None` when auto-detection has not
    /// run yet.
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
