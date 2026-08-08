//! Node selection and group resolution result types.
//!
//! These types are produced by the application-layer selection resolver
//! (`deve_sub_application::template::selection`) when resolving a
//! [`TemplateDocument`]'s `nodeSelector` and `proxyGroups` against the live
//! node pool. They capture which nodes were selected, which referenced nodes
//! are missing, and which quick-group members were auto-populated.
//!
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"Proxy group
//! model" and §"Generation pipeline" for the authoritative flow.

use deve_sub_kernel::NodeId;

/// Why a referenced node was not available for selection or group membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingReason {
    /// The node ID does not exist in the pool at all.
    NotFound,
    /// The node exists but is marked `missing_from_source` (its source removed
    /// it in a prior refresh). Excluded from generation per NODE-011.
    MissingFromSource,
    /// The node exists but is inactive (manually disabled via override).
    Inactive,
}

impl std::fmt::Display for MissingReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not_found"),
            Self::MissingFromSource => write!(f, "missing_from_source"),
            Self::Inactive => write!(f, "inactive"),
        }
    }
}

/// A node reference that could not be resolved to an active pool entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingNodeRef {
    /// The node ID that was referenced.
    pub node_id: NodeId,
    /// Why the node is unavailable.
    pub reason: MissingReason,
}

/// Resolution of a single proxy group's membership against the live pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupResolution {
    /// The group name from the template spec.
    pub group_name: String,
    /// Node IDs from explicit `GroupMember::Node` entries that were found and
    /// active in the pool. Order matches the spec's `members` order.
    pub explicit_node_ids: Vec<NodeId>,
    /// Node IDs auto-populated by the group's `QuickGroupFilter`, in pool
    /// order. Empty when no filter is set.
    pub quick_group_node_ids: Vec<NodeId>,
    /// Explicit `GroupMember::Node` references that could not be resolved.
    pub missing: Vec<MissingNodeRef>,
}

/// Full resolution of a template's node selector and all proxy groups against
/// the live pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateResolution {
    /// Node IDs selected by the template's `nodeSelector` (dynamic or fixed).
    pub selected_node_ids: Vec<NodeId>,
    /// Node IDs from the selector that were referenced but unavailable.
    pub selection_missing: Vec<MissingNodeRef>,
    /// Per-group resolution for each `ProxyGroup` in the spec.
    pub groups: Vec<GroupResolution>,
}
