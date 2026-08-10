//! Node override, tag, and region application commands.
//!
//! These functions orchestrate [`NodeOverrideRepository`] and
//! [`NodePoolRepository`] ports. They do not execute SQL directly. One API
//! operation maps to one command. See `docs/plan/03-architecture.md`
//! §"Lightweight CQRS" and `docs/plan/milestones/M4-sources-and-node-pool.md`
//! §"NODE-004/005/006/010".

use deve_sub_domain::{
    NodeChain, NodeChainError, NodeChainGraph, NodeOverride, NodeOverrideRepository, NodePoolEntry,
    NodePoolRepository, RegionAssignment, RegionMethod, SourceError, Tag,
};
use deve_sub_kernel::{NodeId, NodeOverrideId, TagId};

use super::error::SourceAppError;

/// Maximum tag name length.
const MAX_TAG_NAME_LEN: usize = 128;

/// Parameters for [`update_override`].
pub struct UpdateOverrideParams {
    /// Override display name; `None` keeps the parsed name.
    pub display_name: Option<String>,
    /// Override region; `None` keeps the auto-detected region.
    pub region: Option<String>,
    /// Override enabled flag; `None` keeps the node's natural status.
    pub enabled: Option<bool>,
    /// Override SNI; `None` keeps the parsed TLS SNI.
    pub sni: Option<String>,
    /// Override skip-cert-verify; `None` keeps the parsed value.
    pub skip_cert_verify: Option<bool>,
    /// Override TLS fingerprint; `None` keeps the parsed value.
    pub fingerprint: Option<String>,
    /// Sort order for generation. Higher = later in the output.
    pub sort_order: i64,
}

/// Create or fully replace a node's override (NODE-010).
///
/// Verifies the node exists, then upserts a full [`NodeOverride`] built from
/// `params`. Returns [`SourceAppError::NodeNotFound`] if the node does not
/// exist.
///
/// # Errors
/// - [`SourceAppError::NodeNotFound`] — node does not exist.
/// - [`SourceAppError::Source`] — storage error.
pub async fn update_override(
    override_repo: &dyn NodeOverrideRepository,
    pool_repo: &dyn NodePoolRepository,
    node_id: NodeId,
    params: UpdateOverrideParams,
) -> Result<NodeOverride, SourceAppError> {
    // WHY: fetch only to verify existence; the override is a full-replace
    // built from params, not from the existing entry.
    pool_repo
        .get_node(node_id)
        .await
        .map_err(map_source_error)?
        .ok_or(SourceAppError::NodeNotFound)?;

    let ov = NodeOverride {
        id: NodeOverrideId::new(),
        node_id,
        display_name: params.display_name,
        region: params.region,
        enabled: params.enabled,
        sni: params.sni,
        skip_cert_verify: params.skip_cert_verify,
        fingerprint: params.fingerprint,
        sort_order: params.sort_order,
    };

    override_repo
        .upsert_override(&ov)
        .await
        .map_err(map_source_error)?;
    Ok(ov)
}

/// Delete a node's override. No-op if none exists.
///
/// # Errors
/// - [`SourceAppError::Source`] — storage error.
pub async fn delete_override(
    override_repo: &dyn NodeOverrideRepository,
    node_id: NodeId,
) -> Result<(), SourceAppError> {
    override_repo
        .delete_override(node_id)
        .await
        .map_err(map_source_error)
}

/// Set or clear a node's manual region (NODE-006).
///
/// Passing `None` clears the manual region, reverting to auto-detection.
/// Returns the effective [`RegionAssignment`] after the change: `Manual` when
/// the override carries a region, otherwise `Auto` with the node's stored
/// region.
///
/// # Errors
/// - [`SourceAppError::NodeNotFound`] — node does not exist.
/// - [`SourceAppError::Source`] — storage error.
pub async fn set_manual_region(
    override_repo: &dyn NodeOverrideRepository,
    pool_repo: &dyn NodePoolRepository,
    node_id: NodeId,
    region: Option<String>,
) -> Result<RegionAssignment, SourceAppError> {
    pool_repo
        .get_node(node_id)
        .await
        .map_err(map_source_error)?
        .ok_or(SourceAppError::NodeNotFound)?;

    override_repo
        .patch_override_region(node_id, region)
        .await
        .map_err(map_source_error)?;

    let entry = pool_repo
        .get_node(node_id)
        .await
        .map_err(map_source_error)?
        .ok_or(SourceAppError::NodeNotFound)?;

    Ok(effective_region(&entry))
}

/// Set or clear a node's proxy chain (NODE-017 / NODE-018).
///
/// Passing `None` clears the chain (direct connection). Passing `Some(nodes)`
/// validates the chain structure (non-empty, no self-reference, no duplicates),
/// verifies every referenced node exists in the pool, then runs cycle
/// detection across the entire chain graph before persisting.
///
/// # Errors
/// - [`SourceAppError::NodeNotFound`] — the target node does not exist.
/// - [`SourceAppError::NodeChain`] — structural, existence, or cycle
///   validation failed.
/// - [`SourceAppError::Source`] — storage error.
pub async fn set_node_chain(
    pool_repo: &dyn NodePoolRepository,
    node_id: NodeId,
    chain: Option<Vec<NodeId>>,
) -> Result<Option<Vec<NodeId>>, SourceAppError> {
    // WHY: fetch only to verify existence before writing.
    pool_repo
        .get_node(node_id)
        .await
        .map_err(map_source_error)?
        .ok_or(SourceAppError::NodeNotFound)?;

    let chain = match chain {
        None => {
            pool_repo
                .set_node_chain(node_id, None)
                .await
                .map_err(map_source_error)?;
            return Ok(None);
        }
        Some(nodes) => {
            let node_chain = NodeChain { nodes };
            node_chain.validate_structure(node_id)?;
            node_chain
        }
    };

    let mut missing: Vec<NodeId> = Vec::new();
    for &target in &chain.nodes {
        if pool_repo
            .get_node(target)
            .await
            .map_err(map_source_error)?
            .is_none()
        {
            missing.push(target);
        }
    }
    if !missing.is_empty() {
        return Err(NodeChainError::NodeNotFound(missing).into());
    }

    // WHY: cycle detection must see the would-be graph — the current chains
    // of all nodes, with this node's chain replaced by the candidate. A cycle
    // involving this node would only appear with the new edge(s).
    let mut all_chains = pool_repo
        .list_node_chains()
        .await
        .map_err(map_source_error)?;
    match all_chains.iter().position(|(id, _)| *id == node_id) {
        Some(i) => all_chains[i].1 = chain.nodes.clone(),
        None => all_chains.push((node_id, chain.nodes.clone())),
    }
    let pairs: Vec<(NodeId, Option<Vec<NodeId>>)> = all_chains
        .into_iter()
        .map(|(id, nodes)| (id, Some(nodes)))
        .collect();
    let graph = NodeChainGraph::from_chains(&pairs);
    if let Some(cycle) = graph.detect_cycle() {
        return Err(NodeChainError::Cycle(cycle).into());
    }

    pool_repo
        .set_node_chain(node_id, Some(&chain.nodes))
        .await
        .map_err(map_source_error)?;

    Ok(Some(chain.nodes))
}

/// Batch set the `enabled` flag for multiple nodes (NODE-004).
///
/// Returns the number of rows affected.
///
/// # Errors
/// - [`SourceAppError::Source`] — storage error.
pub async fn batch_set_enabled(
    override_repo: &dyn NodeOverrideRepository,
    node_ids: Vec<NodeId>,
    enabled: bool,
) -> Result<u64, SourceAppError> {
    override_repo
        .batch_set_enabled(&node_ids, enabled)
        .await
        .map_err(map_source_error)
}

/// Replace the tag set for a single node (NODE-005).
///
/// # Errors
/// - [`SourceAppError::Source`] — storage error.
pub async fn set_node_tags(
    override_repo: &dyn NodeOverrideRepository,
    node_id: NodeId,
    tag_ids: Vec<TagId>,
) -> Result<(), SourceAppError> {
    override_repo
        .set_node_tags(node_id, &tag_ids)
        .await
        .map_err(map_source_error)
}

/// Batch replace tags for multiple nodes in one transaction (NODE-005).
///
/// # Errors
/// - [`SourceAppError::Source`] — storage error.
pub async fn batch_set_tags(
    override_repo: &dyn NodeOverrideRepository,
    assignments: Vec<(NodeId, Vec<TagId>)>,
) -> Result<(), SourceAppError> {
    override_repo
        .batch_set_tags(&assignments)
        .await
        .map_err(map_source_error)
}

/// List all tags, ordered by name.
///
/// # Errors
/// - [`SourceAppError::Source`] — storage error.
pub async fn list_tags(
    override_repo: &dyn NodeOverrideRepository,
) -> Result<Vec<Tag>, SourceAppError> {
    override_repo.list_tags().await.map_err(map_source_error)
}

/// Create a new tag.
///
/// Validates the name (non-empty, max 128 chars). The domain layer reports
/// name collisions via [`SourceError::TagExists`], preserved inside
/// [`SourceAppError::Source`].
///
/// # Errors
/// - [`SourceAppError::InvalidInput`] — name empty or too long.
/// - [`SourceAppError::Source`] — storage error (including tag name collision).
pub async fn create_tag(
    override_repo: &dyn NodeOverrideRepository,
    name: &str,
    color: Option<&str>,
) -> Result<Tag, SourceAppError> {
    validate_tag_name(name)?;
    override_repo
        .create_tag(name, color)
        .await
        .map_err(map_source_error)
}

/// Delete a tag by ID. Cascades to `node_tags` via FK.
///
/// # Errors
/// - [`SourceAppError::Source`] — storage error (including tag not found).
pub async fn delete_tag(
    override_repo: &dyn NodeOverrideRepository,
    tag_id: TagId,
) -> Result<(), SourceAppError> {
    override_repo
        .delete_tag(tag_id)
        .await
        .map_err(map_source_error)
}

/// Validate a tag name at the application boundary.
fn validate_tag_name(name: &str) -> Result<(), SourceAppError> {
    if name.is_empty() {
        return Err(SourceAppError::InvalidInput("tag name must not be empty"));
    }
    if name.len() > MAX_TAG_NAME_LEN {
        return Err(SourceAppError::InvalidInput(
            "tag name must not exceed 128 characters",
        ));
    }
    Ok(())
}

/// Compute the effective [`RegionAssignment`] for a pool entry.
///
/// If the override carries a manual region, the method is `Manual`; otherwise
/// the node's stored (auto-detected) region is used with method `Auto`.
fn effective_region(entry: &NodePoolEntry) -> RegionAssignment {
    match entry
        .override_info
        .as_ref()
        .and_then(|ov| ov.region.as_ref())
    {
        Some(region) => RegionAssignment {
            method: RegionMethod::Manual,
            value: Some(region.clone()),
        },
        None => RegionAssignment {
            method: RegionMethod::Auto,
            value: entry.node.region.value.clone(),
        },
    }
}

/// Map storage errors to application errors. Mirrors `commands.rs`: source
/// name collisions map to the flat application variant; all other domain
/// errors (including `TagNotFound` and `TagExists`) preserve their typed form
/// inside [`SourceAppError::Source`].
fn map_source_error(e: SourceError) -> SourceAppError {
    match e {
        SourceError::NameExists => SourceAppError::NameExists,
        other => SourceAppError::Source(other),
    }
}
