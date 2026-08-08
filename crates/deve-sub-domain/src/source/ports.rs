//! Port traits for source storage.

use async_trait::async_trait;

use deve_sub_kernel::{NodeId, Revision, SourceId, SourceSnapshotId, Timestamp};

use super::error::SourceError;
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

    /// Import a batch of pre-parsed nodes manually (NODE-001/002/003).
    ///
    /// Each node is deduplicated against the active pool by
    /// `(protocol_kind, host, port)`: new nodes are inserted, duplicates
    /// are counted but not overwritten (the existing node's credentials
    /// are preserved — NODE-003). No source binding is created; manual
    /// nodes exist in the pool without a `node_source_bindings` row.
    /// The entire batch is committed atomically.
    async fn import_nodes(&self, nodes: Vec<Node>) -> Result<ImportResult, SourceError>;
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
