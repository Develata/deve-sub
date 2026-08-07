//! Node override and tag domain entities.
//!
//! A [`NodeOverride`] is a human-authored patch layered on top of the
//! upstream-parsed [`Node`]. The override does not mutate the original node;
//! the effective node is `parsed_node.apply_override(override)` at read time.
//! Overrides persist across source refreshes (NODE-010) because reconcile
//! never touches the `node_overrides` table.
//!
//! A [`Tag`] is a user-defined label for node grouping and filtering. Tags
//! are many-to-many via the `node_tags` junction table.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use deve_sub_kernel::{NodeId, NodeOverrideId, TagId};

use crate::source::SourceError;

/// Manual override for a node, layered on top of the parsed node.
///
/// All `Option` fields are `None` when not overridden; the effective value
/// falls back to the parsed node's value. The `sort_order` field is always
/// present (defaults to 0) and controls generation ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeOverride {
    /// Unique override ID.
    pub id: NodeOverrideId,
    /// The node this override applies to.
    pub node_id: NodeId,
    /// Override display name; `None` keeps the parsed name.
    pub display_name: Option<String>,
    /// Override region; `None` keeps the auto-detected or parsed region.
    /// When `Some`, the region method becomes `Manual` (NODE-006).
    pub region: Option<String>,
    /// Override enabled flag; `None` keeps the node's natural active status.
    /// `Some(true)` forces active, `Some(false)` forces inactive (NODE-004).
    pub enabled: Option<bool>,
    /// Override SNI; `None` keeps the parsed TLS SNI.
    pub sni: Option<String>,
    /// Override skip-cert-verify; `None` keeps the parsed value.
    pub skip_cert_verify: Option<bool>,
    /// Override TLS fingerprint; `None` keeps the parsed value.
    pub fingerprint: Option<String>,
    /// Sort order for generation. Higher = later in the output. Default 0.
    pub sort_order: i64,
}

/// User-defined tag for node grouping and filtering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    /// Unique tag ID.
    pub id: TagId,
    /// Human-readable tag name (unique).
    pub name: String,
    /// Optional color for UI display (e.g. `"#ff0000"`).
    pub color: Option<String>,
}

/// Storage boundary for node overrides and tags.
///
/// Implementations handle the `node_overrides`, `tags`, and `node_tags`
/// tables. The [`NodePoolRepository`] read path LEFT JOINs `node_overrides`
/// and `node_tags` to reconstruct the effective [`NodePoolEntry`], so
/// override and tag data are returned via the pool query, not via separate
/// calls to this trait.
#[async_trait]
pub trait NodeOverrideRepository: Send + Sync {
    /// Full-replace upsert of an override for the given node.
    async fn upsert_override(&self, ov: &NodeOverride) -> Result<(), SourceError>;

    /// Get the override for a node, if one exists.
    async fn get_override(&self, node_id: NodeId) -> Result<Option<NodeOverride>, SourceError>;

    /// Delete the override for a node. No-op if none exists.
    async fn delete_override(&self, node_id: NodeId) -> Result<(), SourceError>;

    /// Partial update: set only the `region` field, preserving other override
    /// fields. Passing `None` clears the manual region (NODE-006).
    async fn patch_override_region(
        &self,
        node_id: NodeId,
        region: Option<String>,
    ) -> Result<(), SourceError>;

    /// Batch set the `enabled` flag for multiple nodes. Upserts only the
    /// `enabled` column, preserving other override fields (NODE-004).
    /// Returns the number of rows affected.
    async fn batch_set_enabled(
        &self,
        node_ids: &[NodeId],
        enabled: bool,
    ) -> Result<u64, SourceError>;

    /// Replace the tag set for a single node (NODE-005).
    async fn set_node_tags(&self, node_id: NodeId, tag_ids: &[TagId]) -> Result<(), SourceError>;

    /// Batch replace tags for multiple nodes in one transaction (NODE-005).
    async fn batch_set_tags(&self, assignments: &[(NodeId, Vec<TagId>)])
    -> Result<(), SourceError>;

    /// List all tags, ordered by name.
    async fn list_tags(&self) -> Result<Vec<Tag>, SourceError>;

    /// Create a new tag. Returns [`SourceError::TagExists`] on name collision.
    async fn create_tag(&self, name: &str, color: Option<&str>) -> Result<Tag, SourceError>;

    /// Delete a tag by ID. Cascades to `node_tags` via FK. Returns
    /// [`SourceError::TagNotFound`] if the tag does not exist.
    async fn delete_tag(&self, tag_id: TagId) -> Result<(), SourceError>;
}
