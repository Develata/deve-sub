//! Port traits for source storage.

use async_trait::async_trait;

use deve_sub_kernel::{NodeId, Revision, SourceId, SourceSnapshotId, Timestamp};

use super::error::SourceError;
use super::refresh_job::{RefreshPhase, SourceRefreshJob};
use super::source_item::ItemParseStatus;
use super::{Source, SourceSnapshot};
use crate::node_override::{NodeOverride, Tag};
use crate::{Node, ProtocolKind};

/// Storage boundary for source aggregates.
#[async_trait]
pub trait SourceRepository: Send + Sync {
    /// Create a new source. Returns [`SourceError::NameExists`] if the name
    /// is already taken.
    async fn create(&self, source: &Source) -> Result<(), SourceError>;

    /// Find a source by ID.
    async fn find_by_id(&self, id: SourceId) -> Result<Option<Source>, SourceError>;

    /// Find a source by name.
    async fn find_by_name(&self, name: &str) -> Result<Option<Source>, SourceError>;

    /// List sources with cursor pagination.
    ///
    /// Returns up to `limit` sources whose ULID is strictly greater than
    /// `cursor` (or all sources if `cursor` is `None`), ordered by `id`.
    async fn list(&self, cursor: Option<SourceId>, limit: u32) -> Result<Vec<Source>, SourceError>;

    /// Update an existing source. Returns [`SourceError::SourceNotFound`]
    /// if the source does not exist.
    async fn update(&self, source: &Source) -> Result<(), SourceError>;

    /// Delete a source and all its snapshots, items, and source bindings.
    async fn delete(&self, id: SourceId) -> Result<(), SourceError>;
}

/// Storage boundary for source snapshots.
#[async_trait]
pub trait SourceSnapshotRepository: Send + Sync {
    /// Create a new snapshot and deactivate the previous active snapshot
    /// for the same source in a single transaction.
    async fn create(&self, snapshot: &SourceSnapshot) -> Result<(), SourceError>;

    /// Find the active snapshot for a source.
    async fn find_active(&self, source_id: SourceId)
    -> Result<Option<SourceSnapshot>, SourceError>;

    /// Find the active snapshot for each of the given source IDs in a single
    /// query (batch fetch).
    ///
    /// Returns a map keyed by [`SourceId`]. Sources without an active snapshot
    /// are absent from the result. Used by the scheduler to check all due
    /// sources in one query instead of N `find_active` calls.
    async fn find_active_for_sources(
        &self,
        source_ids: &[SourceId],
    ) -> Result<std::collections::HashMap<SourceId, SourceSnapshot>, SourceError>;

    /// List snapshots for a source, newest first.
    async fn list_for_source(
        &self,
        source_id: SourceId,
        limit: u32,
    ) -> Result<Vec<SourceSnapshot>, SourceError>;

    /// Find a snapshot by ID.
    async fn find_by_id(&self, id: SourceSnapshotId)
    -> Result<Option<SourceSnapshot>, SourceError>;
}

/// One entry from a parsed source refresh, ready for reconciliation.
///
/// The application layer creates one `ReconcileEntry` per parsed item. The
/// reconciler inserts a [`SourceItem`] for each entry and upserts the node
/// into the pool when `node` is `Some`.
#[derive(Debug, Clone)]
pub struct ReconcileEntry {
    /// Raw text of the entry (share URI or serialized fragment).
    pub raw_uri: String,
    /// Initial parse status from the parser. The reconciler may upgrade
    /// `Parsed` to `Duplicate` if the node already exists in the pool.
    pub initial_status: ItemParseStatus,
    /// Parsed node, if the entry produced one. `None` for `Failed` entries.
    /// `Some` for `Parsed` and `Unsupported` entries.
    pub node: Option<Node>,
}

/// Input for [`NodePoolRepository::reconcile`].
#[derive(Debug, Clone)]
pub struct ReconcileInput<'a> {
    /// The source being refreshed.
    pub source_id: SourceId,
    /// The new snapshot to create (deactivates the previous active one).
    pub snapshot: &'a SourceSnapshot,
    /// Parsed entries from the fetched content.
    pub entries: &'a [ReconcileEntry],
}

/// Result of a successful reconciliation.
#[derive(Debug, Clone, Default)]
pub struct ReconcileResult {
    /// Nodes newly inserted into the pool.
    pub new_nodes: u64,
    /// Nodes already in the pool (duplicate of an existing active node).
    pub duplicate_nodes: u64,
    /// Nodes that were missing and have been reactivated.
    pub reactivated_nodes: u64,
    /// Nodes previously bound to this source that are no longer present.
    pub missing_nodes: u64,
}

/// A node plus its pool-side metadata, returned by node pool queries.
///
/// The [`Node`] aggregate carries protocol/endpoint/config but no
/// pool metadata. This wrapper adds the columns the `nodes` table tracks
/// separately: `missing_from_source`, `is_active`, `revision`, and
/// `created_at`. See migration 0004 and `docs/data-model/core-er.md`.
#[derive(Debug, Clone)]
pub struct NodePoolEntry {
    /// The canonical node aggregate.
    pub node: Node,
    /// Whether the node was marked missing after its last source removed it.
    /// Missing nodes stay in the pool for diagnostics; they are excluded
    /// from generation. See NODE-011.
    pub missing_from_source: bool,
    /// Whether the node is active (eligible for generation). Defaults to
    /// `true` on insert. Manual disable lands with NODE-004.
    pub is_active: bool,
    /// Optimistic-concurrency revision counter, bumped on each update.
    pub revision: u64,
    /// Row creation time (distinct from the ULID's embedded time and from
    /// `Node::source.imported_at`).
    pub created_at: Timestamp,

    /// Manual override applied to this node, if any. `None` when no
    /// `node_overrides` row exists. The effective node is
    /// `parsed_node.apply_override(override)`. See NODE-010.
    pub override_info: Option<NodeOverride>,

    /// Tags assigned to this node, resolved from the `node_tags` junction.
    pub tags: Vec<Tag>,
}

/// Filters applied to node pool queries.
///
/// All fields optional; `None` means no filter on that dimension. Used by
/// [`NodePoolRepository::list_nodes`] and the `/api/v1/nodes` route.
#[derive(Debug, Clone, Default)]
pub struct NodeFilter {
    /// Only nodes with this protocol kind.
    pub protocol: Option<ProtocolKind>,
    /// Only nodes whose region equals this value (case-sensitive).
    pub region: Option<String>,
    /// Only nodes whose `missing_from_source` matches.
    pub include_missing: bool,
    /// Only nodes whose `is_active` matches.
    pub include_inactive: bool,
}

impl NodeFilter {
    /// A filter matching active, non-missing nodes only — the default view
    /// for generation.
    #[must_use]
    pub fn active_only() -> Self {
        Self {
            protocol: None,
            region: None,
            include_missing: false,
            include_inactive: false,
        }
    }

    /// A filter matching every node in the pool regardless of state.
    #[must_use]
    pub fn all() -> Self {
        Self {
            protocol: None,
            region: None,
            include_missing: true,
            include_inactive: true,
        }
    }
}

/// Result of a manual node import (NODE-001 / NODE-002).
///
/// Each input URI is classified into one of three outcomes: newly inserted,
/// duplicate of an existing active node, or failed to parse. The counts
/// mirror the per-row `ItemParseStatus` used by source refresh so the UI
/// and CLI can render a single import summary.
#[derive(Debug, Clone, Default)]
pub struct ImportResult {
    /// Nodes newly inserted into the pool.
    pub new_nodes: u64,
    /// Nodes already present (same protocol + host + port, active and
    /// non-missing). Not re-inserted; credentials are NOT overwritten
    /// (NODE-003: do not drop nodes with different credentials).
    pub duplicate_nodes: u64,
    /// Input lines that could not be parsed at all.
    pub failed: u64,
    /// Per-line outcomes for diagnostics. Length equals the input line
    /// count; order preserved. Failed lines carry the raw URI; successful
    /// lines carry the resulting `NodeId`.
    pub outcomes: Vec<ImportOutcome>,
}

/// Per-line outcome of a manual import.
#[derive(Debug, Clone)]
pub enum ImportOutcome {
    /// A new node was inserted; carries its `NodeId`.
    Inserted(NodeId),
    /// The node was a duplicate of an existing active pool entry; carries
    /// the existing node's `NodeId`.
    Duplicate(NodeId),
    /// The line could not be parsed; carries the raw input text.
    Failed(String),
}

/// Storage boundary for the node pool and source reconciliation.
///
/// The [`reconcile`] method performs the entire refresh transaction:
/// deactivate the previous snapshot, create the new one, insert source
/// items, dedup and upsert nodes, create source bindings, and mark missing
/// nodes. All in a single database transaction (constraint #19: on failure,
/// preserve the last successful subscription version).
///
/// Query methods ([`list_nodes`], [`get_node`]) return [`NodePoolEntry`]
/// including pool metadata. [`import_nodes`] inserts manually-provided
/// nodes with dedup but no source binding (NODE-001/002/003).
#[async_trait]
pub trait NodePoolRepository: Send + Sync {
    /// Reconcile a source refresh: create snapshot, insert items, upsert
    /// nodes, mark missing. Atomic — either the entire refresh commits or
    /// nothing changes.
    async fn reconcile(&self, input: ReconcileInput<'_>) -> Result<ReconcileResult, SourceError>;

    /// List nodes from the pool, optionally filtered, with cursor
    /// pagination by `NodeId`.
    ///
    /// Returns up to `limit` entries whose `NodeId` is strictly greater
    /// than `cursor` (or all if `cursor` is `None`), ordered by `id`.
    /// `filter` selects protocol/region/active/missing subsets.
    async fn list_nodes(
        &self,
        filter: &NodeFilter,
        cursor: Option<NodeId>,
        limit: u32,
    ) -> Result<Vec<NodePoolEntry>, SourceError>;

    /// Get a single node by ID, including pool metadata.
    ///
    /// Returns `None` if no node with the given ID exists.
    async fn get_node(&self, id: NodeId) -> Result<Option<NodePoolEntry>, SourceError>;

    /// Get multiple nodes by ID in a single query (batch fetch).
    ///
    /// Returns one [`NodePoolEntry`] per found ID. IDs that do not exist are
    /// simply absent from the result. The result order is not guaranteed;
    /// callers that need order should index by [`NodeId`]. Used to avoid
    /// N+1 queries in compatibility checks and generation pipelines.
    async fn get_nodes(&self, ids: &[NodeId]) -> Result<Vec<NodePoolEntry>, SourceError>;

    /// Import a batch of pre-parsed nodes manually (NODE-001/002/003).
    ///
    /// Each node is deduplicated against the active pool by
    /// `(protocol_kind, host, port)`: new nodes are inserted, duplicates
    /// are counted but not overwritten (the existing node's credentials
    /// are preserved — NODE-003). No source binding is created; manual
    /// nodes exist in the pool without a `node_source_bindings` row.
    /// The entire batch is committed atomically.
    async fn import_nodes(&self, nodes: Vec<Node>) -> Result<ImportResult, SourceError>;

    /// List all node chains in the pool. Returns one [`NodeChainEntry`] per
    /// node that has a non-null, non-empty chain. Used for cycle detection
    /// (NODE-018) before persisting a new chain.
    async fn list_node_chains(&self) -> Result<Vec<crate::NodeChainEntry>, SourceError>;

    /// Return the subset of `ids` that exist in the node pool. Used for
    /// batch existence checks (e.g. validating chain references in one
    /// query instead of N `get_node` calls). Order of the result is not
    /// guaranteed; callers that need order should sort or index.
    async fn existing_node_ids(&self, ids: &[NodeId]) -> Result<Vec<NodeId>, SourceError>;

    /// Set or clear a single node's chain (NODE-017). `chain = None` clears
    /// the column (direct connection); `Some(vec)` persists the ordered
    /// node IDs as a JSON array. The caller is responsible for structural
    /// validation and cycle detection before calling.
    async fn set_node_chain(
        &self,
        node_id: NodeId,
        chain: Option<&[NodeId]>,
    ) -> Result<(), SourceError>;
}

/// Storage boundary for the global pool revision counter.
///
/// The pool revision is a monotonic counter bumped on every node pool
/// mutation (reconcile, import). It serves as a cache-key component for
/// subscription generation so stale cache entries are invalidated when the
/// pool changes. See `docs/plan/milestones/M5-generator-and-v3-template.md`
/// §"Generation cache" and `deve-sub-kernel::Revision`.
#[async_trait]
pub trait PoolMetaRepository: Send + Sync {
    /// Read the current pool revision.
    async fn get_revision(&self) -> Result<Revision, SourceError>;

    /// Atomically bump the pool revision by one and return the new value.
    async fn bump_revision(&self) -> Result<Revision, SourceError>;
}

/// Storage boundary for source refresh jobs (B-15).
///
/// Each refresh attempt creates one job row. The per-source lease is
/// enforced by a partial UNIQUE index at the DB level: at most one
/// `Running` job per source. The runner creates a job (Pending → Running),
/// updates the phase as it progresses, and writes a terminal status
/// (Completed / Failed / Cancelled) when done.
#[async_trait]
pub trait SourceRefreshJobRepository: Send + Sync {
    /// Create a new refresh job row in `Pending` status.
    ///
    /// Returns [`SourceError::RefreshInProgress`] if a `Running` job already
    /// exists for this source (the lease is held). This is the application-
    /// level lease check; the DB-level partial unique index is the final
    /// defense-in-depth guarantee.
    async fn create(&self, job: &SourceRefreshJob) -> Result<(), SourceError>;

    /// Find a refresh job by ID.
    async fn find_by_id(
        &self,
        id: deve_sub_kernel::SourceRefreshJobId,
    ) -> Result<Option<SourceRefreshJob>, SourceError>;

    /// Find the Running job for a source, if any. Used by the scheduler to
    /// check whether a source is already being refreshed before starting a
    /// new refresh (the lease check).
    async fn find_running_for_source(
        &self,
        source_id: SourceId,
    ) -> Result<Option<SourceRefreshJob>, SourceError>;

    /// Transition a job to `Running` status. Called by the runner when it
    /// picks up the job. Returns [`SourceError::RefreshInProgress`] if
    /// another job is already Running for this source (lease contention).
    async fn mark_running(
        &self,
        id: deve_sub_kernel::SourceRefreshJobId,
    ) -> Result<(), SourceError>;

    /// Update the job's current phase (progress indicator). Called before
    /// each phase: fetching, parsing, enriching, reconciling, publishing.
    /// Also refreshes `started_at` as a heartbeat so the scheduler's
    /// stale-lease sweep does not kill a long-running but active job
    /// (P0-10).
    async fn update_phase(
        &self,
        id: deve_sub_kernel::SourceRefreshJobId,
        phase: RefreshPhase,
    ) -> Result<(), SourceError>;

    /// Mark a job as completed with reconcile counts and not_modified flag.
    async fn mark_completed(
        &self,
        id: deve_sub_kernel::SourceRefreshJobId,
        new_nodes: u64,
        duplicate_nodes: u64,
        reactivated_nodes: u64,
        missing_nodes: u64,
        not_modified: bool,
    ) -> Result<(), SourceError>;

    /// Mark a job as failed with an error message.
    async fn mark_failed(
        &self,
        id: deve_sub_kernel::SourceRefreshJobId,
        error_message: &str,
    ) -> Result<(), SourceError>;

    /// Mark a job as cancelled. Called when a cancel signal is received or
    /// on shutdown. Best-effort — the job may have already completed.
    async fn mark_cancelled(
        &self,
        id: deve_sub_kernel::SourceRefreshJobId,
    ) -> Result<(), SourceError>;

    /// List recent refresh jobs for a source, newest first.
    async fn list_for_source(
        &self,
        source_id: SourceId,
        limit: u32,
    ) -> Result<Vec<SourceRefreshJob>, SourceError>;

    /// Mark all jobs left in `Pending` or `Running` status as `Failed`
    /// (crash recovery on startup, constraint #20).
    ///
    /// WHY: if the process crashes mid-refresh, the job row stays in
    /// `Running` forever. Because the per-source lease is enforced by a
    /// partial UNIQUE index on `(source_id) WHERE status = 'R'`, a stuck
    /// Running row blocks all future refreshes for that source. This method
    /// releases those orphaned leases. Returns the count of recovered jobs.
    async fn recover_crashed_jobs(&self) -> Result<u64, SourceError>;

    /// Mark `Running` jobs whose `started_at` is older than `cutoff` as
    /// `Failed` (scheduler tick lease sweep).
    ///
    /// WHY: a refresh that has not progressed in `max_age` is presumed dead
    /// — the runner task may have been killed by the OS, panicked without
    /// unwinding, or the host lost power. Unlike
    /// [`recover_crashed_jobs`](Self::recover_crashed_jobs) (a blanket
    /// startup sweep), this is a bounded age-based check run at each
    /// scheduler tick so a hung job does not hold the lease for the entire
    /// uptime. Returns the count of recovered jobs.
    ///
    /// `started_at` is refreshed on each phase transition (heartbeat), so
    /// `cutoff` effectively means "no phase transition in this duration" —
    /// not "total job duration" (P0-10).
    async fn recover_stale_jobs(&self, cutoff: Timestamp, reason: &str)
    -> Result<u64, SourceError>;
}
