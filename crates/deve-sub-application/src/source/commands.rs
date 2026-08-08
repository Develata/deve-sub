//! Source application commands and queries: create, update, delete, list, get.
//!
//! These functions orchestrate domain services and port interfaces. They do
//! not execute SQL directly. One API operation maps to one command. See
//! `docs/plan/03-architecture.md` §"Lightweight CQRS".

use deve_sub_domain::{
    ImportResult, NodeFilter, NodePoolEntry, NodePoolRepository, ProtocolKind, ReconcileInput,
    ReconcileResult, Source, SourceError, SourceFilterRules, SourceRepository, SourceSnapshot,
    SourceSnapshotRepository, SourceType,
};
use deve_sub_kernel::{NodeId, SourceId, SourceSnapshotId, Timestamp};

use super::error::SourceAppError;
use super::fetcher::{FetchResult, SubscriptionFetcher};
use super::filter::{apply_protocol_filter, apply_region_filter};
use super::geoip::{GeoIpPort, enrich_regions};
use super::parse::parse_content;

/// Maximum source name length.
const MAX_NAME_LEN: usize = 128;

/// Maximum URL length.
const MAX_URL_LEN: usize = 2048;

/// Maximum update interval in seconds (30 days).
const MAX_UPDATE_INTERVAL_SECS: u64 = 30 * 24 * 3600;

/// Validate a source name at the application boundary.
fn validate_name(name: &str) -> Result<(), SourceAppError> {
    if name.is_empty() {
        return Err(SourceAppError::InvalidInput("name must not be empty"));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(SourceAppError::InvalidInput(
            "name must not exceed 128 characters",
        ));
    }
    Ok(())
}

/// Validate a source URL at the application boundary.
fn validate_url(url: &str) -> Result<(), SourceAppError> {
    if url.is_empty() {
        return Err(SourceAppError::InvalidInput("url must not be empty"));
    }
    if url.len() > MAX_URL_LEN {
        return Err(SourceAppError::InvalidInput(
            "url must not exceed 2048 characters",
        ));
    }
    // WHY: reject obviously non-HTTP schemes to catch user typos early. The
    // fetcher performs full SSRF validation before connecting; this check is
    // a fast pre-filter at the application boundary.
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(SourceAppError::InvalidInput(
            "url must start with http:// or https://",
        ));
    }
    Ok(())
}

/// Parameters for [`create_source`].
pub struct CreateSourceParams {
    /// Human-readable name.
    pub name: String,
    /// Input format. `Auto` lets the fetcher detect.
    pub source_type: SourceType,
    /// Subscription URL.
    pub url: String,
    /// Whether automatic refresh is enabled.
    pub auto_update: bool,
    /// Refresh interval in seconds.
    pub update_interval_secs: u64,
    /// Whether to keep existing nodes if a refresh fails.
    pub keep_on_fail: bool,
    /// Include/exclude filter rules applied to parsed nodes (SRC-010).
    pub filter_rules: Option<SourceFilterRules>,
}

/// Create a new subscription source.
///
/// Validates input, constructs a [`Source`] domain object, and persists it.
/// Returns [`SourceAppError::NameExists`] if the name is already taken.
///
/// # Errors
/// - [`SourceAppError::InvalidInput`] — validation failed.
/// - [`SourceAppError::NameExists`] — name collision.
/// - [`SourceAppError::Source`] — storage error.
pub async fn create_source(
    repo: &dyn SourceRepository,
    params: CreateSourceParams,
) -> Result<Source, SourceAppError> {
    validate_name(&params.name)?;
    validate_url(&params.url)?;
    if params.update_interval_secs == 0 {
        return Err(SourceAppError::InvalidInput(
            "update_interval_secs must be greater than 0",
        ));
    }
    if params.update_interval_secs > MAX_UPDATE_INTERVAL_SECS {
        return Err(SourceAppError::InvalidInput(
            "update_interval_secs must not exceed 30 days",
        ));
    }

    let mut source = Source::new(&params.name, params.source_type, params.url);
    source.auto_update = params.auto_update;
    source.update_interval_secs = params.update_interval_secs;
    source.keep_on_fail = params.keep_on_fail;
    source.filter_rules = params.filter_rules;

    repo.create(&source).await.map_err(map_source_error)?;
    Ok(source)
}

/// Parameters for [`update_source`].
pub struct UpdateSourceParams {
    /// ID of the source to update.
    pub id: SourceId,
    /// New name.
    pub name: String,
    /// New input format.
    pub source_type: SourceType,
    /// New subscription URL.
    pub url: String,
    /// Whether automatic refresh is enabled.
    pub auto_update: bool,
    /// Refresh interval in seconds.
    pub update_interval_secs: u64,
    /// Whether the source is active.
    pub enabled: bool,
    /// Whether to keep existing nodes if a refresh fails.
    pub keep_on_fail: bool,
    /// Include/exclude filter rules applied to parsed nodes (SRC-010).
    pub filter_rules: Option<SourceFilterRules>,
}

/// Update an existing source.
///
/// Loads the source, applies the new fields, and persists it. Returns
/// [`SourceAppError::SourceNotFound`] if the source does not exist.
///
/// # Errors
/// - [`SourceAppError::InvalidInput`] — validation failed.
/// - [`SourceAppError::SourceNotFound`] — source does not exist.
/// - [`SourceAppError::NameExists`] — name collision.
/// - [`SourceAppError::Source`] — storage error.
pub async fn update_source(
    repo: &dyn SourceRepository,
    params: UpdateSourceParams,
) -> Result<Source, SourceAppError> {
    validate_name(&params.name)?;
    validate_url(&params.url)?;
    if params.update_interval_secs == 0 {
        return Err(SourceAppError::InvalidInput(
            "update_interval_secs must be greater than 0",
        ));
    }
    if params.update_interval_secs > MAX_UPDATE_INTERVAL_SECS {
        return Err(SourceAppError::InvalidInput(
            "update_interval_secs must not exceed 30 days",
        ));
    }

    let mut source = repo
        .find_by_id(params.id)
        .await
        .map_err(map_source_error)?
        .ok_or(SourceAppError::SourceNotFound)?;

    source.name = params.name;
    source.source_type = params.source_type;
    source.url = params.url;
    source.auto_update = params.auto_update;
    source.update_interval_secs = params.update_interval_secs;
    source.enabled = params.enabled;
    source.keep_on_fail = params.keep_on_fail;
    source.filter_rules = params.filter_rules;

    repo.update(&source).await.map_err(map_source_error)?;
    Ok(source)
}

/// Delete a source by ID.
///
/// Returns [`SourceAppError::SourceNotFound`] if the source does not exist.
/// The storage layer cascades the deletion to snapshots, items, and
/// node-source bindings.
///
/// # Errors
/// - [`SourceAppError::SourceNotFound`] — source does not exist.
/// - [`SourceAppError::Source`] — storage error.
pub async fn delete_source(
    repo: &dyn SourceRepository,
    id: SourceId,
) -> Result<(), SourceAppError> {
    repo.delete(id).await.map_err(map_delete_error)?;
    Ok(())
}

/// Get a source by ID.
///
/// # Errors
/// - [`SourceAppError::Source`] — storage error.
pub async fn get_source(
    repo: &dyn SourceRepository,
    id: SourceId,
) -> Result<Option<Source>, SourceAppError> {
    repo.find_by_id(id).await.map_err(map_source_error)
}

/// List sources with cursor pagination.
///
/// Returns up to `limit` sources whose ULID is greater than `cursor` (or all
/// if `cursor` is `None`). The caller derives the next cursor from the last
/// element's ID.
///
/// # Errors
/// - [`SourceAppError::Source`] — storage error.
pub async fn list_sources(
    repo: &dyn SourceRepository,
    cursor: Option<SourceId>,
    limit: u32,
) -> Result<Vec<Source>, SourceAppError> {
    repo.list(cursor, limit).await.map_err(map_source_error)
}

/// Result of a successful source refresh.
#[derive(Debug, Clone)]
pub struct RefreshResult {
    /// The snapshot created by this refresh.
    pub snapshot: SourceSnapshot,
    /// Reconciliation counts from the node pool update.
    pub reconcile: ReconcileResult,
    /// Whether the fetch returned 304 Not Modified. When `true`, no new
    /// snapshot was created and `snapshot` refers to the previously active
    /// one.
    pub not_modified: bool,
}

/// Refresh a source: fetch → parse → reconcile.
///
/// Fetches the subscription URL (SSRF-checked by the fetcher), parses the
/// content, and reconciles the parsed nodes into the pool in a single
/// transaction. On fetch or parse failure, the last successful snapshot
/// remains active (constraint #19). When `source.keep_on_fail` is `false`,
/// a fetch or parse failure also disables the source (sets `enabled = false`)
/// per plan M4 §"Failure/recovery"; the admin can re-enable it after fixing
/// the URL.
///
/// # Errors
/// - [`SourceAppError::SourceNotFound`] — source does not exist.
/// - [`SourceAppError::Fetch`] — the fetch failed (SSRF, timeout, HTTP error).
/// - [`SourceAppError::Parse`] — the content could not be parsed.
/// - [`SourceAppError::Source`] — storage or reconciliation error.
pub async fn refresh_source(
    source_repo: &dyn SourceRepository,
    snapshot_repo: &dyn SourceSnapshotRepository,
    pool_repo: &dyn NodePoolRepository,
    fetcher: &dyn SubscriptionFetcher,
    geoip: &dyn GeoIpPort,
    source_id: SourceId,
) -> Result<RefreshResult, SourceAppError> {
    let source = source_repo
        .find_by_id(source_id)
        .await
        .map_err(map_source_error)?
        .ok_or(SourceAppError::SourceNotFound)?;

    let active = snapshot_repo
        .find_active(source_id)
        .await
        .map_err(map_source_error)?;
    let etag = active.as_ref().and_then(|s| s.etag.clone());

    let fetch = match fetcher.fetch(&source.url, etag.as_deref()).await {
        Ok(f) => f,
        Err(e) => {
            disable_on_failure(source_repo, &source).await;
            return Err(e.into());
        }
    };

    if let FetchResult::NotModified = fetch {
        let snapshot = active.ok_or(SourceAppError::Source(SourceError::Storage(
            "server returned 304 but no active snapshot exists".to_owned(),
        )))?;
        return Ok(RefreshResult {
            snapshot,
            reconcile: ReconcileResult::default(),
            not_modified: true,
        });
    }

    let (body, resp_etag, content_type) = match fetch {
        FetchResult::Ok {
            body,
            etag,
            content_type,
        } => (body, etag, content_type),
        FetchResult::NotModified => {
            // WHY: the NotModified arm above returns early; this arm exists
            // only to satisfy the exhaustive match and can never execute.
            return Err(SourceAppError::Source(SourceError::Storage(
                "unreachable: NotModified after 304 check".to_owned(),
            )));
        }
    };

    let mut entries = match parse_content(source.source_type, content_type.as_deref(), &body) {
        Ok(e) => e,
        Err(e) => {
            disable_on_failure(source_repo, &source).await;
            return Err(e.into());
        }
    };

    // WHY: SRC-006 — if parse yielded zero valid nodes and an active snapshot
    // already exists, preserve it rather than creating a new zero-node snapshot
    // that would mark all existing nodes as missing. A transient empty response
    // (server error page, format change) must not wipe the node pool.
    let valid_after_parse = entries.iter().filter(|e| e.node.is_some()).count();
    if valid_after_parse == 0 && active.is_some() {
        return Err(SourceAppError::ZeroNodes);
    }

    // WHY: apply protocol filter (SRC-010 phase 1) before region enrichment
    // so filtered nodes do not consume GeoIP lookups. Protocol is known at
    // parse time, so this phase is safe to run before enrich_regions.
    if let Some(ref rules) = source.filter_rules {
        apply_protocol_filter(&mut entries, rules);
    }

    // WHY: auto-detect regions via GeoIP before reconcile. Manual overrides
    // in `node_overrides` take precedence at read time (NODE-006/010), so
    // this only sets the parsed node's stored region, not the effective one.
    enrich_regions(&mut entries, geoip).await;

    // WHY: apply region filter (SRC-010 phase 2) after region enrichment so
    // region rules match against the GeoIP-detected region. Running before
    // enrichment would see region=None for all nodes, making region rules
    // non-functional.
    if let Some(ref rules) = source.filter_rules {
        apply_region_filter(&mut entries, rules);
    }

    let new_version = active.as_ref().map(|s| s.version + 1).unwrap_or(1);
    let node_count =
        u64::try_from(entries.iter().filter(|e| e.node.is_some()).count()).map_err(|_| {
            SourceAppError::Source(SourceError::Storage("node count overflow".to_owned()))
        })?;

    let snapshot = SourceSnapshot {
        id: SourceSnapshotId::new(),
        source_id,
        version: new_version,
        fetched_at: Timestamp::now(),
        etag: resp_etag,
        node_count,
        is_active: true,
    };

    let input = ReconcileInput {
        source_id,
        snapshot: &snapshot,
        entries: &entries,
    };
    let reconcile = pool_repo.reconcile(input).await.map_err(map_source_error)?;

    Ok(RefreshResult {
        snapshot,
        reconcile,
        not_modified: false,
    })
}

/// Best-effort disable a source after a refresh failure when `keep_on_fail`
/// is false.
///
/// WHY: plan M4 §"Failure/recovery" requires that when `keep_on_fail` is
/// false, a failed refresh marks the source as errored. We map "errored" to
/// `enabled = false` because the `jobs` table (for detailed error recording)
/// is not yet built; a proper `errored` status is deferred to the
/// jobs-table milestone.
async fn disable_on_failure(repo: &dyn SourceRepository, source: &Source) {
    if !source.keep_on_fail && source.enabled {
        let mut disabled = source.clone();
        disabled.enabled = false;
        if let Err(e) = repo.update(&disabled).await {
            tracing::warn!(error = %e, "failed to disable source after refresh failure");
        }
    }
}

/// Map storage errors to application errors for non-delete operations.
fn map_source_error(e: SourceError) -> SourceAppError {
    match e {
        SourceError::NameExists => SourceAppError::NameExists,
        other => SourceAppError::Source(other),
    }
}

/// Map storage errors for delete operations. Delete returns
/// `SourceNotFound` on zero rows affected, which maps to the application
/// error directly.
fn map_delete_error(e: SourceError) -> SourceAppError {
    match e {
        SourceError::SourceNotFound => SourceAppError::SourceNotFound,
        SourceError::NameExists => SourceAppError::NameExists,
        other => SourceAppError::Source(other),
    }
}

/// Filter parameters for [`list_nodes`], mirroring [`NodeFilter`] plus
/// pagination.
#[derive(Debug, Clone, Default)]
pub struct ListNodesParams {
    /// Filter by protocol kind.
    pub protocol: Option<ProtocolKind>,
    /// Filter by region (case-sensitive exact match).
    pub region: Option<String>,
    /// Include nodes marked missing from their source.
    pub include_missing: bool,
    /// Include inactive (disabled) nodes.
    pub include_inactive: bool,
    /// Pagination cursor — the ULID of the last node from the previous page.
    pub cursor: Option<NodeId>,
    /// Maximum number of nodes to return.
    pub limit: u32,
}

impl ListNodesParams {
    fn to_filter(&self) -> NodeFilter {
        NodeFilter {
            protocol: self.protocol.clone(),
            region: self.region.clone(),
            include_missing: self.include_missing,
            include_inactive: self.include_inactive,
        }
    }
}

/// List nodes from the pool with optional filters and cursor pagination.
///
/// # Errors
/// - [`SourceAppError::Source`] — storage error.
pub async fn list_nodes(
    pool_repo: &dyn NodePoolRepository,
    params: ListNodesParams,
) -> Result<Vec<NodePoolEntry>, SourceAppError> {
    pool_repo
        .list_nodes(&params.to_filter(), params.cursor, params.limit)
        .await
        .map_err(map_source_error)
}

/// Get a single node by ID, including pool metadata.
///
/// # Errors
/// - [`SourceAppError::Source`] — storage error.
pub async fn get_node(
    pool_repo: &dyn NodePoolRepository,
    id: NodeId,
) -> Result<Option<NodePoolEntry>, SourceAppError> {
    pool_repo.get_node(id).await.map_err(map_source_error)
}

/// Import a batch of pre-parsed nodes into the pool (NODE-001/002/003).
///
/// Deduplicates by `(protocol_kind, host, port)`; duplicates are counted but
/// not overwritten. See [`NodePoolRepository::import_nodes`].
///
/// # Errors
/// - [`SourceAppError::Source`] — storage error.
pub async fn import_nodes(
    pool_repo: &dyn NodePoolRepository,
    nodes: Vec<deve_sub_domain::Node>,
) -> Result<ImportResult, SourceAppError> {
    pool_repo
        .import_nodes(nodes)
        .await
        .map_err(map_source_error)
}
