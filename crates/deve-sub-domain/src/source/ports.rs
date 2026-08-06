//! Port traits for source storage.

use async_trait::async_trait;

use deve_sub_kernel::{SourceId, SourceSnapshotId};

use super::error::SourceError;
use super::{Source, SourceSnapshot};

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
