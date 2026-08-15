//! Assembled template IR: the fully-resolved, profile-agnostic input to
//! container emitters.
//!
//! The application layer builds an [`AssembledTemplate`] from the template
//! spec + resolved node pool, then passes it to the appropriate emitter
//! (`emit_mihomo_full`, etc.). The emitter maps it to the target format's
//! document structure.
//!
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"Generation
//! pipeline" step "assemble ProxyGroups".

use deve_sub_domain::{GroupType, Node};

/// Fully-resolved template IR passed to container emitters.
///
/// Carries:
/// - `nodes`: canonical nodes (compatibility-filtered, sorted, deduped)
/// - `groups`: proxy groups with resolved member display names
/// - `rules`: routing rule objects (opaque JSON, profile-specific shape)
/// - `dns`, `tun`, `output`: profile-specific config (opaque JSON)
///
/// The emitter maps each field to the target format's document structure.
/// Fields that a target format does not support are ignored (with a pipeline
/// warning, not a silent drop — constraint #7).
#[derive(Debug, Clone)]
pub struct AssembledTemplate {
    /// Compatibility-filtered, sorted, deduped canonical nodes.
    pub nodes: Vec<Node>,
    /// Proxy groups with members resolved to display names.
    pub groups: Vec<AssembledGroup>,
    /// Routing rules (opaque JSON values; shape varies by target profile).
    pub rules: Vec<serde_json::Value>,
    /// Profile-specific DNS config (opaque JSON, null if absent).
    pub dns: serde_json::Value,
    /// Profile-specific TUN config (opaque JSON, null if absent).
    pub tun: serde_json::Value,
    /// Profile-specific output options (opaque JSON, null if absent).
    pub output: serde_json::Value,
}

impl AssembledTemplate {
    /// Build an IR with only nodes (no groups, rules, dns, tun, or output).
    ///
    /// Used by profiles that do not yet support group/rule emission and by
    /// tests that only exercise proxy emission.
    #[must_use]
    pub fn from_nodes(nodes: Vec<Node>) -> Self {
        Self {
            nodes,
            groups: Vec::new(),
            rules: Vec::new(),
            dns: serde_json::Value::Null,
            tun: serde_json::Value::Null,
            output: serde_json::Value::Null,
        }
    }
}

/// A proxy group with resolved member display names.
///
/// `members` contains node display names (matching the `name` field in the
/// emitted `proxies:` section) and/or other group names. For `Direct` and
/// `Reject` group types, `members` is typically empty — the emitter maps
/// these to built-in policies.
#[derive(Debug, Clone)]
pub struct AssembledGroup {
    /// Unique group name.
    pub name: String,
    /// Group behavior type.
    pub group_type: GroupType,
    /// Resolved member display names (node names and/or group references).
    pub members: Vec<String>,
}
