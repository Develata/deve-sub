//! Source snapshot entity.

use deve_sub_kernel::{SourceId, SourceSnapshotId, Timestamp};

/// A point-in-time record of a source refresh.
///
/// Each refresh creates a snapshot recording the ETag, node count, and
/// whether it is the active snapshot. Only one snapshot per source is
/// active at a time; the previous snapshot is deactivated when a new one
/// is committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSnapshot {
    /// Unique identifier (ULID).
    pub id: SourceSnapshotId,
    /// The source this snapshot belongs to.
    pub source_id: SourceId,
    /// Monotonically increasing version number per source.
    pub version: u64,
    /// When the snapshot was created.
    pub fetched_at: Timestamp,
    /// ETag from the HTTP response, if any.
    pub etag: Option<String>,
    /// Number of nodes parsed from the fetched content.
    pub node_count: u64,
    /// Whether this is the active snapshot for the source.
    pub is_active: bool,
}
