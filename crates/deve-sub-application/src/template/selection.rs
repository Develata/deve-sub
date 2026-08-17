//! Node selection and proxy-group resolution against the live node pool.
//!
//! This module resolves a [`TemplateDocument`]'s `nodeSelector` and
//! `proxyGroups` into concrete node IDs by querying the
//! [`NodePoolRepository`]. It handles:
//!
//! - **Dynamic selection**: apply `NodeFilterRule`s to all active, non-missing
//!   pool entries. New nodes that match the filters are automatically included
//!   (GEN-005).
//! - **Fixed selection**: look up pinned `node_ids` individually. New nodes are
//!   not included because they are not in the pinned list (GEN-006).
//! - **Quick-group filters**: auto-populate group members by region, protocol,
//!   or tag (GEN-007, GEN-008).
//! - **Missing reference detection**: report node references that are not
//!   found, missing from source, or inactive (GEN-011).
//! - **Sort order**: apply ascending/descending alphabetical sort to resolved
//!   members.
//!
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"Proxy group
//! model" and §"Generation pipeline".

use deve_sub_domain::source::{NodeFilter, NodePoolEntry, NodePoolRepository};
use deve_sub_domain::template::{
    FilterField, GroupMember, GroupResolution, MissingNodeRef, MissingReason, NodeFilterRule,
    NodeSelector, ProxyGroup, QuickGroupFilter, SelectionMode, SortOrder, TemplateDocument,
    TemplateResolution,
};
use deve_sub_kernel::NodeId;

use super::error::TemplateAppError;

/// Page size for pool listing. Large enough to cover typical deployments in
/// one or two pages; the loop continues until exhausted.
const POOL_PAGE_SIZE: u32 = 1000;

/// Resolve a template's node selector and all proxy groups against the live
/// pool.
///
/// This is a read-only operation: it queries the pool and returns which nodes
/// are selected, which group members are resolved, and which references are
/// missing. It does not generate output or modify state.
pub async fn resolve_template(
    doc: &TemplateDocument,
    pool_repo: &dyn NodePoolRepository,
) -> Result<TemplateResolution, TemplateAppError> {
    let active_pool = list_active_nodes(pool_repo).await?;

    // WHY: collect every explicit/pinned NodeId across the selector and all
    // proxy groups, then issue a single batch `get_nodes` query instead of
    // one `get_node` per reference (W-D: K+1 pool listing + N get_node →
    // 1 list + 1 batch fetch).
    let mut explicit_ids: Vec<NodeId> = doc.spec.node_selector.node_ids.clone();
    for group in &doc.spec.proxy_groups {
        for member in &group.members {
            if let GroupMember::Node { id } = member {
                explicit_ids.push(*id);
            }
        }
    }
    explicit_ids.sort_unstable();
    explicit_ids.dedup();
    let explicit_entries = fetch_explicit_entries(pool_repo, &explicit_ids).await?;
    let view = PoolView {
        active: &active_pool,
        explicit: &explicit_entries,
    };

    let (selected_node_ids, selection_missing) =
        resolve_selection_impl(&doc.spec.node_selector, &view);

    let mut groups = Vec::with_capacity(doc.spec.proxy_groups.len());
    for group in &doc.spec.proxy_groups {
        let resolution = resolve_group_impl(group, &view);
        groups.push(resolution);
    }

    Ok(TemplateResolution {
        selected_node_ids,
        selection_missing,
        groups,
    })
}

/// Resolve the template's `nodeSelector` against the pool.
///
/// - **Dynamic**: list all active, non-missing nodes and apply filter rules.
///   Matching node IDs are returned in pool order (by `NodeId`).
/// - **Fixed**: look up each pinned `node_id` individually. Found and active
///   nodes are returned; missing ones are reported.
pub async fn resolve_selection(
    selector: &NodeSelector,
    pool_repo: &dyn NodePoolRepository,
) -> Result<(Vec<NodeId>, Vec<MissingNodeRef>), TemplateAppError> {
    let active_pool = list_active_nodes(pool_repo).await?;
    let explicit_entries = fetch_explicit_entries(pool_repo, &selector.node_ids).await?;
    let view = PoolView {
        active: &active_pool,
        explicit: &explicit_entries,
    };
    Ok(resolve_selection_impl(selector, &view))
}

/// Resolve a single proxy group's membership against the pool.
///
/// Explicit `GroupMember::Node` entries are checked individually. If a
/// `QuickGroupFilter` is present, matching nodes from the pool are appended
/// (deduplicated against explicit members). `GroupMember::Group` references are
/// not resolved here — they are validated structurally in `validate_document`.
pub async fn resolve_group(
    group: &ProxyGroup,
    pool_repo: &dyn NodePoolRepository,
) -> Result<GroupResolution, TemplateAppError> {
    let active_pool = list_active_nodes(pool_repo).await?;
    let explicit_ids: Vec<NodeId> = group
        .members
        .iter()
        .filter_map(|m| match m {
            GroupMember::Node { id } => Some(*id),
            _ => None,
        })
        .collect();
    let explicit_entries = fetch_explicit_entries(pool_repo, &explicit_ids).await?;
    let view = PoolView {
        active: &active_pool,
        explicit: &explicit_entries,
    };
    Ok(resolve_group_impl(group, &view))
}

/// Pre-fetched pool data shared by the synchronous `_impl` resolvers.
///
/// `active` is the full list of active, non-missing pool entries (for dynamic
/// selection and quick-group filtering). `explicit` is a lookup of specific
/// node IDs requested by name (for fixed selection and explicit group members).
/// Both are fetched once by the async entry points and reused across all
/// groups, avoiding K+1 pool listings and N individual `get_node` calls.
struct PoolView<'a> {
    active: &'a [NodePoolEntry],
    explicit: &'a std::collections::HashMap<NodeId, NodePoolEntry>,
}

fn resolve_selection_impl(
    selector: &NodeSelector,
    view: &PoolView<'_>,
) -> (Vec<NodeId>, Vec<MissingNodeRef>) {
    match selector.mode {
        SelectionMode::Dynamic => {
            let filtered: Vec<NodeId> = view
                .active
                .iter()
                .filter(|e| matches_all_filters(e, &selector.filters))
                .map(|e| e.node.id)
                .collect();
            (filtered, Vec::new())
        }
        SelectionMode::Fixed => {
            let mut found = Vec::with_capacity(selector.node_ids.len());
            let mut missing = Vec::new();
            for id in &selector.node_ids {
                match view.explicit.get(id) {
                    Some(entry) if entry.is_active && !entry.missing_from_source => {
                        found.push(*id);
                    }
                    Some(entry) if entry.missing_from_source => {
                        missing.push(MissingNodeRef {
                            node_id: *id,
                            reason: MissingReason::MissingFromSource,
                        });
                    }
                    Some(_) => {
                        missing.push(MissingNodeRef {
                            node_id: *id,
                            reason: MissingReason::Inactive,
                        });
                    }
                    None => {
                        missing.push(MissingNodeRef {
                            node_id: *id,
                            reason: MissingReason::NotFound,
                        });
                    }
                }
            }
            (found, missing)
        }
    }
}

fn resolve_group_impl(group: &ProxyGroup, view: &PoolView<'_>) -> GroupResolution {
    let mut explicit_node_ids = Vec::new();
    let mut missing = Vec::new();
    let mut explicit_set: std::collections::HashSet<NodeId> = std::collections::HashSet::new();

    for member in &group.members {
        if let GroupMember::Node { id } = member {
            match view.explicit.get(id) {
                Some(entry) if entry.is_active && !entry.missing_from_source => {
                    explicit_node_ids.push(*id);
                    explicit_set.insert(*id);
                }
                Some(entry) if entry.missing_from_source => {
                    missing.push(MissingNodeRef {
                        node_id: *id,
                        reason: MissingReason::MissingFromSource,
                    });
                }
                Some(_) => {
                    missing.push(MissingNodeRef {
                        node_id: *id,
                        reason: MissingReason::Inactive,
                    });
                }
                None => {
                    missing.push(MissingNodeRef {
                        node_id: *id,
                        reason: MissingReason::NotFound,
                    });
                }
            }
        }
    }

    let mut quick_group_node_ids = Vec::new();
    if let Some(filter) = &group.filter {
        for entry in view.active {
            if explicit_set.contains(&entry.node.id) {
                continue;
            }
            if matches_quick_group(entry, filter) {
                quick_group_node_ids.push(entry.node.id);
            }
        }
    }

    GroupResolution {
        group_name: group.name.clone(),
        explicit_node_ids,
        quick_group_node_ids,
        missing,
    }
}

/// Batch-fetch node entries by ID, returning a lookup map.
///
/// Empty input returns an empty map without hitting the repository.
async fn fetch_explicit_entries(
    pool_repo: &dyn NodePoolRepository,
    ids: &[NodeId],
) -> Result<std::collections::HashMap<NodeId, NodePoolEntry>, TemplateAppError> {
    if ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let entries = pool_repo
        .get_nodes(ids)
        .await
        .map_err(|e| TemplateAppError::Storage(e.to_string()))?;
    Ok(entries.into_iter().map(|e| (e.node.id, e)).collect())
}

/// Apply sort order to a list of node IDs using the display names from the
/// pool entries.
///
/// `SortOrder::Latency` is not yet supported (latency data arrives with
/// url-test probes in a later slice); it falls back to ascending alphabetical
/// order for now.
pub fn apply_sort_order(node_ids: &mut [NodeId], entries: &[NodePoolEntry], sort_order: SortOrder) {
    let name_by_id: std::collections::HashMap<NodeId, &str> = entries
        .iter()
        .map(|e| (e.node.id, e.node.display_name.as_str()))
        .collect();

    node_ids.sort_by(|a, b| {
        let name_a = name_by_id.get(a).copied().unwrap_or("");
        let name_b = name_by_id.get(b).copied().unwrap_or("");
        match sort_order {
            SortOrder::Asc | SortOrder::Latency => name_a.cmp(name_b),
            SortOrder::Desc => name_b.cmp(name_a),
        }
    });
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// List all active, non-missing nodes from the pool, paginating until
/// exhausted.
async fn list_active_nodes(
    pool_repo: &dyn NodePoolRepository,
) -> Result<Vec<NodePoolEntry>, TemplateAppError> {
    let filter = NodeFilter::active_only();
    let mut all = Vec::new();
    let mut cursor: Option<NodeId> = None;
    loop {
        let page = pool_repo
            .list_nodes(&filter, cursor, POOL_PAGE_SIZE)
            .await
            .map_err(|e| TemplateAppError::Storage(e.to_string()))?;
        if page.is_empty() {
            break;
        }
        cursor = page.last().map(|e| e.node.id);
        all.extend(page);
        if cursor.is_none() {
            break;
        }
    }
    Ok(all)
}

/// Check whether a pool entry matches all filter rules (AND semantics).
fn matches_all_filters(entry: &NodePoolEntry, rules: &[NodeFilterRule]) -> bool {
    rules.iter().all(|r| matches_filter_rule(entry, r))
}

/// Check whether a pool entry matches a single filter rule.
fn matches_filter_rule(entry: &NodePoolEntry, rule: &NodeFilterRule) -> bool {
    match rule.field {
        FilterField::Protocol => entry
            .node
            .protocol
            .as_filter_key()
            .eq_ignore_ascii_case(&rule.value),
        FilterField::Region => entry
            .node
            .region
            .value
            .as_deref()
            .unwrap_or("")
            .eq_ignore_ascii_case(&rule.value),
        FilterField::Tag => entry
            .tags
            .iter()
            .any(|t| t.name.eq_ignore_ascii_case(&rule.value)),
        FilterField::Source => entry
            .node
            .source
            .source_label
            .eq_ignore_ascii_case(&rule.value),
    }
}

/// Check whether a pool entry matches a quick-group filter (all set fields
/// must match, AND semantics).
fn matches_quick_group(entry: &NodePoolEntry, filter: &QuickGroupFilter) -> bool {
    if let Some(region) = &filter.region
        && !entry
            .node
            .region
            .value
            .as_deref()
            .unwrap_or("")
            .eq_ignore_ascii_case(region)
    {
        return false;
    }
    if let Some(protocol) = &filter.protocol
        && !entry
            .node
            .protocol
            .as_filter_key()
            .eq_ignore_ascii_case(protocol)
    {
        return false;
    }
    if let Some(tag) = &filter.tag
        && !entry.tags.iter().any(|t| t.name.eq_ignore_ascii_case(tag))
    {
        return false;
    }
    true
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
