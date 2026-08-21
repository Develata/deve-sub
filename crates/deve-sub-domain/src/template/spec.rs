//! V3 template spec value objects.
//!
//! The `TemplateSpec` is the declarative YAML body a user authors, matching
//! `apiVersion: deve-sub.io/v1` / `kind: SubscriptionTemplate`. It is stored
//! verbatim as YAML in the `template_versions` table and validated by the
//! application layer before persistence.
//!
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"V3 Template
//! schema" for the authoritative schema.

use deve_sub_kernel::NodeId;
use serde::{Deserialize, Serialize};

/// The V3 API version string.
pub const API_VERSION: &str = "deve-sub.io/v1";

/// The template kind string.
pub const KIND: &str = "SubscriptionTemplate";

/// Maximum allowed serialized spec size (1 MiB, SEC-005 parity).
pub const MAX_SPEC_BYTES: usize = 1024 * 1024;

/// Maximum YAML alias nesting depth (SEC-005 parity).
pub const MAX_ALIAS_DEPTH: u32 = 10;

/// The seven proxy group types (spec §11.2). The concrete set available to a
/// given template is filtered by the target profile's capability matrix at
/// generation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroupType {
    /// Manual selection.
    Select,
    /// Lowest-latency auto-selection.
    UrlTest,
    /// First-available failover.
    Fallback,
    /// Load-balanced distribution.
    LoadBalance,
    /// Sequential chain (relay) — participates in the chain graph.
    Relay,
    /// Direct (bypass proxy).
    Direct,
    /// Reject (block traffic).
    Reject,
}

impl std::fmt::Display for GroupType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Select => write!(f, "select"),
            Self::UrlTest => write!(f, "url-test"),
            Self::Fallback => write!(f, "fallback"),
            Self::LoadBalance => write!(f, "load-balance"),
            Self::Relay => write!(f, "relay"),
            Self::Direct => write!(f, "direct"),
            Self::Reject => write!(f, "reject"),
        }
    }
}

impl std::str::FromStr for GroupType {
    type Err = super::error::TemplateError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "select" => Ok(Self::Select),
            "url-test" => Ok(Self::UrlTest),
            "fallback" => Ok(Self::Fallback),
            "load-balance" => Ok(Self::LoadBalance),
            "relay" => Ok(Self::Relay),
            "direct" => Ok(Self::Direct),
            "reject" => Ok(Self::Reject),
            other => Err(super::error::TemplateError::InvalidSpec(format!(
                "unknown group type: {other}"
            ))),
        }
    }
}

/// A reference to a member of a proxy group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GroupMember {
    /// A concrete node from the pool.
    Node {
        /// The node's ULID.
        id: NodeId,
    },
    /// A reference to another proxy group by name.
    Group {
        /// The referenced group's name.
        name: String,
    },
}

/// A proxy group definition within a template spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroup {
    /// Unique group name within this template.
    pub name: String,
    /// Group behavior type.
    #[serde(rename = "type")]
    pub group_type: GroupType,
    /// Ordered member list. `relay` groups interpret order as the chain
    /// sequence.
    #[serde(default)]
    pub members: Vec<GroupMember>,
    /// Optional quick-group filter that populates members at generation time.
    /// When present, dynamic members are appended after explicit `members`.
    #[serde(default)]
    pub filter: Option<QuickGroupFilter>,
    /// Optional sort order for rendered members.
    #[serde(default)]
    pub sort_order: Option<SortOrder>,
}

/// A quick-group filter auto-populates group members from the pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickGroupFilter {
    /// Match nodes by region (case-insensitive).
    #[serde(default)]
    pub region: Option<String>,
    /// Match nodes by protocol (filter key, e.g. `trojan`).
    #[serde(default)]
    pub protocol: Option<String>,
    /// Match nodes by tag name.
    #[serde(default)]
    pub tag: Option<String>,
}

/// Sort order applied to a group's rendered member list.
///
/// WHY (P0-13): a `Latency` variant existed here but was never implemented —
/// both call sites silently fell back to ascending alphabetical order,
/// violating the "no silent semantic fallback" rule (constraint #7). The
/// variant was removed rather than left as dead public contract.
///
/// No data migration is needed: the project is pre-first-tagged-release (no
/// production install base), no code path ever produced `sort_order: "latency"`,
/// and no migration or seed inserts it. If this enum is later extended with
/// latency-based sort, add a new variant with a round-trip test before
/// exposing it in the public template spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    /// Ascending alphabetical by node display name.
    Asc,
    /// Descending alphabetical.
    Desc,
}

/// Node selection mode for the template.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSelector {
    /// `dynamic` re-evaluates filters at generation time; `fixed` pins a
    /// set of node IDs.
    #[serde(rename = "mode")]
    pub mode: SelectionMode,
    /// Filters for dynamic mode. Ignored in fixed mode.
    #[serde(default)]
    pub filters: Vec<NodeFilterRule>,
    /// Pinned node IDs for fixed mode. Ignored in dynamic mode.
    #[serde(default)]
    pub node_ids: Vec<NodeId>,
    /// Pool revision captured at save time for fixed mode. `0` in dynamic
    /// mode.
    ///
    /// WHY: this field is advisory metadata, not an enforcement point. Fixed
    /// mode resolves `node_ids` against the live pool at generation time
    /// without verifying the revision matches; a node that still exists and
    /// is active is included regardless of how the pool has changed since
    /// save. The revision is retained for audit/display and future
    /// staleness detection, but does not gate generation.
    #[serde(default)]
    pub node_revision: u64,
}

/// Selection mode discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    /// Re-evaluate filters against the live pool at each generation.
    #[default]
    Dynamic,
    /// Use the pinned node IDs, resolved against the live pool at
    /// generation time. `node_revision` is advisory only.
    Fixed,
}

/// A single node filter rule for dynamic selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeFilterRule {
    /// Filter dimension.
    pub field: FilterField,
    /// Match value (string comparison).
    pub value: String,
}

/// Filterable dimensions for node selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterField {
    /// Protocol kind filter key.
    Protocol,
    /// Region (GeoIP-assigned or manual override).
    Region,
    /// User-defined tag.
    Tag,
    /// Source ID the node was imported from.
    Source,
}

/// A routing rule in the template spec.
///
/// Stored as an opaque JSON value because rule shapes vary by target profile
/// and the canonical rule schema is finalized in M5 Slice 5. The application
/// layer validates structure only against the SEC-005 limits (depth, size, no
/// scripts); semantic rule validation runs at generation time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// Raw rule object.
    #[serde(default)]
    pub value: serde_json::Value,
}

/// The full template spec body (the `spec:` block of the V3 document).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSpec {
    /// Target profiles this template can generate for.
    #[serde(default)]
    pub target_profiles: Vec<String>,
    /// User-defined variables for rule/dns/tun interpolation.
    #[serde(default)]
    pub variables: serde_json::Value,
    /// Node selection configuration.
    #[serde(default)]
    pub node_selector: NodeSelector,
    /// Proxy group definitions.
    #[serde(default)]
    pub proxy_groups: Vec<ProxyGroup>,
    /// Routing rules.
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Profile-specific DNS config (opaque JSON).
    #[serde(default)]
    pub dns: serde_json::Value,
    /// Profile-specific TUN config (opaque JSON).
    #[serde(default)]
    pub tun: serde_json::Value,
    /// Profile-specific output options (opaque JSON).
    #[serde(default)]
    pub output: serde_json::Value,
}

/// The V3 template document (metadata + spec).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateDocument {
    /// API version. Must equal [`API_VERSION`].
    pub api_version: String,
    /// Resource kind. Must equal [`KIND`].
    pub kind: String,
    /// Template metadata.
    pub metadata: TemplateMetadata,
    /// Template spec.
    pub spec: TemplateSpec,
}

/// Metadata block of the V3 template document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateMetadata {
    /// Human-readable template name (unique across the deployment).
    pub name: String,
    /// Optional description.
    #[serde(default)]
    pub description: String,
    /// Monotonic, server-assigned version number.
    #[serde(default)]
    pub version: u64,
}
