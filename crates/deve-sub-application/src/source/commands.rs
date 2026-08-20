//! Source application commands and queries: create, update, delete, list, get.
//!
//! These functions orchestrate domain services and port interfaces. They do
//! not execute SQL directly. One API operation maps to one command. See
//! `docs/plan/03-architecture.md` §"Lightweight CQRS".

use deve_sub_domain::{
    ImportResult, NodeFilter, NodePoolEntry, NodePoolRepository, ProtocolKind, Source, SourceError,
    SourceFilterRules, SourceRepository, SourceType,
};
use deve_sub_kernel::{NodeId, SourceId};

use super::error::SourceAppError;

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

pub use super::refresh::RefreshResult;

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
