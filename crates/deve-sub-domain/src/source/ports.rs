//! Port traits for source storage.

use async_trait::async_trait;

use deve_sub_kernel::{SourceId, SourceSnapshotId};

use super::error::SourceError;
use super::source_item::ItemParseStatus;
use super::{Source, SourceSnapshot};
use crate::Node;

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

/// Storage boundary for the node pool and source reconciliation.
///
/// The [`reconcile`] method performs the entire refresh transaction:
/// deactivate the previous snapshot, create the new one, insert source
/// items, dedup and upsert nodes, create source bindings, and mark missing
/// nodes. All in a single database transaction (constraint #19: on failure,
/// preserve the last successful subscription version).
#[async_trait]
pub trait NodePoolRepository: Send + Sync {
    /// Reconcile a source refresh: create snapshot, insert items, upsert
    /// nodes, mark missing. Atomic — either the entire refresh commits or
    /// nothing changes.
    async fn reconcile(&self, input: ReconcileInput<'_>) -> Result<ReconcileResult, SourceError>;
}
