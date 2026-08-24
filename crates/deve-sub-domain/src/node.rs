//! The canonical [`Node`] aggregate and its supporting types.
//!
//! See ADR-0003 for the canonical node model decision and
//! `docs/plan/05-protocol-engine.md` for the full blueprint. This module
//! depends only on [`deve_sub_kernel`] and sibling domain modules.

use std::collections::BTreeMap;
use std::collections::HashSet;

use deve_sub_kernel::{NodeId, TagId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::NodeChainError;
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
    /// Node-level proxy chain: route traffic through a sequence of nodes
    /// before reaching this node's endpoint. `None` means direct connection.
    /// See M7 plan §"Node chain proxy" and NODE-017/018.
    pub chain: Option<NodeChain>,
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
    /// Hysteria2, Trojan, Shadowsocks password. The Shadowsocks cipher
    /// `method` is protocol configuration, carried by [`ShadowsocksConfig`].
    Password { password: String },
    /// NaiveProxy username+password.
    UserPassword { username: String, password: String },
    /// TUIC v5 uuid+password.
    UuidPassword { uuid: String, password: String },
    /// No authentication (e.g. unauthed Socks5/HTTP).
    None,
}

/// A node-level proxy chain: an ordered list of node IDs that traffic
/// traverses before reaching this node's endpoint. Serialized as a plain
/// JSON array of ULID strings (`#[serde(transparent)]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeChain {
    /// Ordered node IDs forming the chain. Must be non-empty.
    ///
    /// `pub(crate)` so same-crate tests can construct edge cases (e.g. empty
    /// chains to test `validate_structure` rejection) while cross-crate code
    /// must use [`NodeChain::new`], enforcing the non-empty invariant.
    pub(crate) nodes: Vec<NodeId>,
}

impl NodeChain {
    /// Create a [`NodeChain`] from `nodes`, enforcing the non-empty
    /// invariant. Structural validation (self-reference, duplicates) is
    /// deferred to [`validate_structure`] because it needs `self_id`.
    ///
    /// # Errors
    /// - [`NodeChainError::Empty`] — `nodes` is empty.
    pub fn new(nodes: Vec<NodeId>) -> Result<Self, NodeChainError> {
        if nodes.is_empty() {
            return Err(NodeChainError::Empty);
        }
        Ok(Self { nodes })
    }

    /// Read-only access to the ordered chain node IDs.
    #[must_use]
    pub fn nodes(&self) -> &[NodeId] {
        &self.nodes
    }

    /// Validate the chain's structural invariants (non-empty, no
    /// self-reference, no duplicate entries). Does NOT check node existence
    /// or cycles — those require repository context.
    ///
    /// # Errors
    /// - [`NodeChainError::Empty`] — `nodes` is empty.
    /// - [`NodeChainError::SelfReference`] — `self_id` appears in `nodes`.
    /// - [`NodeChainError::Duplicate`] — `nodes` contains duplicate IDs.
    pub fn validate_structure(&self, self_id: NodeId) -> Result<(), NodeChainError> {
        if self.nodes.is_empty() {
            return Err(NodeChainError::Empty);
        }
        if self.nodes.contains(&self_id) {
            return Err(NodeChainError::SelfReference);
        }
        let mut seen = HashSet::new();
        for &id in &self.nodes {
            if !seen.insert(id) {
                return Err(NodeChainError::Duplicate(id));
            }
        }
        Ok(())
    }
}

/// Server-derived enrichment keys stored in `node.extras` that must NOT
/// participate in identity fingerprinting (B-12).
///
/// WHY: unlike protocol-specific extras (which distinguish functionally
/// different proxies), these keys are recomputed by the server on every
/// refresh from external state — `candidate_ips` mirrors DNS resolution
/// output, whose order rotates with resolver behavior and whose membership
/// rotates with CDN DNS. Including them would change the fingerprint on an
/// unchanged node, so each refresh would insert a new pool row and mark the
/// old one missing, churning every downstream NodeId binding (tags, chains,
/// overrides, traffic history) whenever DNS answers differ.
const NON_IDENTITY_EXTRAS: &[&str] = &["candidate_ips"];

/// Identity-relevant fields of a [`Node`], used for deduplication (B-12).
///
/// Serialized via `serde_json::Value` (BTreeMap-backed by default, since the
/// workspace does not enable serde_json's `preserve_order` feature) to
/// produce canonical JSON with alphabetically sorted keys, independent of
/// struct field declaration order. The resulting string is the input to the
/// keyed HMAC identity fingerprint.
///
/// WHY: `id`, `display_name`, `source`, `region`, `tags`, and `chain` are
/// excluded — they are assigned by the pool, user-facing metadata, or
/// negotiated at runtime, and do not distinguish otherwise-identical proxy
/// endpoints. Two nodes that differ ONLY in these fields are the same
/// endpoint and should dedup. `udp`, `multiplex`, `congestion`, and
/// `extras` ARE included (P0-08): two nodes at the same endpoint with the
/// same credentials but different UDP capability, multiplex settings,
/// congestion control, or protocol-specific extra fields are functionally
/// different proxies and must NOT be collapsed.
///
/// Exception: keys listed in [`NON_IDENTITY_EXTRAS`] are stripped from
/// `extras` before serialization.
#[derive(Debug, Clone, Serialize)]
struct NodeIdentityRef<'a> {
    protocol: &'a ProtocolKind,
    endpoint: &'a Endpoint,
    authentication: &'a Authentication,
    config: &'a ProtocolConfig,
    tls: &'a Option<TlsConfig>,
    transport: &'a Option<Transport>,
    obfuscation: &'a Option<Obfuscation>,
    udp: &'a UdpCapability,
    multiplex: &'a Option<MultiplexConfig>,
    congestion: &'a Option<CongestionConfig>,
    extras: &'a BTreeMap<&'a str, &'a serde_json::Value>,
}

impl Node {
    /// Return a canonical JSON string of all identity-relevant fields.
    ///
    /// Two nodes with the same canonical identity are considered the same
    /// proxy endpoint and deduplicated in the pool. Fields that distinguish
    /// endpoints — credentials, SNI, transport path, Reality keys, UDP
    /// capability, multiplex, congestion control, protocol-specific extras —
    /// are included; metadata fields (`id`, `display_name`, `source`,
    /// `region`, `tags`, `chain`) and enrichment keys
    /// ([`NON_IDENTITY_EXTRAS`], e.g. `candidate_ips`) are not.
    ///
    /// Keys are sorted alphabetically by going through `serde_json::Value`
    /// (BTreeMap-backed by default), giving canonical JSON independent of
    /// struct field declaration order. The resulting string is the input to
    /// the keyed HMAC identity fingerprint (B-12).
    ///
    /// See B-12 in the v0.1.0 pre-release audit and NODE-003.
    ///
    /// # Errors
    /// Returns `serde_json::Error` if serialization fails (should not happen
    /// for in-memory `Node` values, but is propagated for correctness).
    pub fn canonical_identity_str(&self) -> Result<String, serde_json::Error> {
        // WHY: enrichment keys in NON_IDENTITY_EXTRAS describe external
        // state (DNS answers), not the node itself; filtering them here (at
        // the single fingerprint input point) keeps the pool dedup stable
        // across refreshes regardless of what enrichment writes.
        let identity_extras: BTreeMap<&str, &serde_json::Value> = self
            .extras
            .iter()
            .filter(|(k, _)| !NON_IDENTITY_EXTRAS.contains(&k.as_str()))
            .map(|(k, v)| (k.as_str(), v))
            .collect();
        let v = serde_json::to_value(NodeIdentityRef {
            protocol: &self.protocol,
            endpoint: &self.endpoint,
            authentication: &self.authentication,
            config: &self.config,
            tls: &self.tls,
            transport: &self.transport,
            obfuscation: &self.obfuscation,
            udp: &self.udp,
            multiplex: &self.multiplex,
            congestion: &self.congestion,
            extras: &identity_extras,
        })?;
        serde_json::to_string(&v)
    }
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
    ///
    /// WHY: `#[serde(skip)]` prevents accidental credential leakage when
    /// `Node` is serialized to JSON (logs, intermediate formats, API DTOs).
    /// The persistence adapter handles storage separately via encrypted
    /// columns; this field is only for in-memory processing.
    #[serde(skip)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::{DomainName, Host};
    use crate::protocol::UnsupportedNode;

    fn minimal_node(extras: BTreeMap<String, serde_json::Value>) -> Node {
        Node {
            id: NodeId::new(),
            display_name: "test".to_owned(),
            protocol: ProtocolKind::Trojan,
            config: ProtocolConfig::Unsupported(UnsupportedNode {
                raw: serde_json::Value::Null,
                raw_format: None,
                reason: "test fixture".to_owned(),
            }),
            endpoint: Endpoint {
                host: Host::Domain(DomainName::new("node.test.example".to_owned())),
                port: 443,
            },
            authentication: Authentication::Password {
                password: "TEST_PASSWORD".to_owned(),
            },
            transport: None,
            tls: None,
            udp: UdpCapability::default(),
            multiplex: None,
            obfuscation: None,
            congestion: None,
            chain: None,
            source: NodeSource {
                source_label: "reserved-test-source".to_owned(),
                raw_uri: None,
                imported_at: Timestamp::now(),
            },
            tags: vec![],
            region: RegionAssignment {
                method: RegionMethod::Auto,
                value: None,
            },
            extras,
        }
    }

    /// Enrichment keys (NON_IDENTITY_EXTRAS) must not change the identity:
    /// DNS-dependent `candidate_ips` variation across refreshes would
    /// otherwise churn the pool fingerprint.
    #[test]
    fn candidate_ips_do_not_affect_identity() {
        let mut extras_a = BTreeMap::new();
        extras_a.insert(
            "candidate_ips".to_owned(),
            serde_json::json!(["192.0.2.10", "2001:db8::1"]),
        );
        let mut extras_b = BTreeMap::new();
        extras_b.insert(
            "candidate_ips".to_owned(),
            serde_json::json!(["2001:db8::25", "198.51.100.7", "203.0.113.9"]),
        );

        let a = minimal_node(extras_a).canonical_identity_str().expect("a");
        let b = minimal_node(extras_b).canonical_identity_str().expect("b");
        assert_eq!(a, b, "candidate_ips must be excluded from identity");
    }

    /// Protocol-specific extras ARE identity: a differing plugin option is a
    /// functionally different proxy and must not dedup against the original.
    #[test]
    fn protocol_extras_do_affect_identity() {
        let mut extras_a = BTreeMap::new();
        extras_a.insert("plugin_opts".to_owned(), serde_json::json!("mode=ws"));
        let mut extras_b = BTreeMap::new();
        extras_b.insert("plugin_opts".to_owned(), serde_json::json!("mode=quic"));

        let a = minimal_node(extras_a).canonical_identity_str().expect("a");
        let b = minimal_node(extras_b).canonical_identity_str().expect("b");
        assert_ne!(a, b, "protocol extras must stay in identity");
    }

    /// Lock the canonical JSON key order (B-12). The fingerprint input must
    /// depend only on VALUES, never on map insertion order — this test fails
    /// if serde_json's `preserve_order` feature is ever enabled workspace
    /// -wide (nested objects would become insertion-ordered IndexMaps).
    #[test]
    fn canonical_identity_object_keys_are_sorted() {
        let mut extras = BTreeMap::new();
        extras.insert(
            "nested".to_owned(),
            serde_json::json!({"z_key": 1, "a_key": 2, "m_key": 3}),
        );
        let canonical = minimal_node(extras)
            .canonical_identity_str()
            .expect("canonical");

        // The nested object's keys must appear sorted in the serialized form.
        let z = canonical.find("\"z_key\"").expect("z_key present");
        let a = canonical.find("\"a_key\"").expect("a_key present");
        let m = canonical.find("\"m_key\"").expect("m_key present");
        assert!(a < m && m < z, "nested object keys must serialize sorted");
    }
}
