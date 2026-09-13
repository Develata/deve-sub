//! Port traits for audit log storage.
//!
//! Events are immutable; bounded deletion requires an atomic cleanup receipt. See
//! `docs/plan/milestones/M10-observability-and-audit.md` §"Audit log model".

use async_trait::async_trait;

use deve_sub_kernel::{AuditLogId, Timestamp, UserId};

use super::AuditLog;
use super::error::AuditError;

/// Filters applied to audit log list queries.
///
/// All fields optional; `None` means no filter on that dimension. Used by
/// [`AuditLogRepository::list`] and the `/api/v1/audit-logs` route.
#[derive(Debug, Clone, Default)]
pub struct AuditLogFilter {
    /// Only entries by this actor.
    pub actor_id: Option<UserId>,
    /// Only entries with this action (e.g. `"auth.login"`).
    pub action: Option<String>,
    /// Only entries with this target type (e.g. `"user"`).
    pub target_type: Option<String>,
    /// Only entries with this target ID.
    pub target_id: Option<String>,
    /// Inclusive UTC start of the time range.
    pub since: Option<Timestamp>,
    /// Exclusive UTC end of the time range.
    pub before: Option<Timestamp>,
}

/// Maximum rows removed by one cleanup transaction (M10 log lifecycle).
pub const AUDIT_CLEANUP_BATCH: usize = 500;

/// A bounded, ordered snapshot of the oldest events before a UTC cutoff.
#[derive(Debug, Clone)]
pub struct AuditCleanupPreview {
    /// Exclusive UTC cutoff.
    pub before: Timestamp,
    /// Exact candidate IDs, ordered by created_at then ID.
    pub entry_ids: Vec<AuditLogId>,
    /// Another batch will be needed after this one.
    pub has_more: bool,
}

/// Storage boundary for the append-only audit log.
#[async_trait]
pub trait AuditLogRepository: Send + Sync {
    /// Append a new audit log entry. The `id` and `created_at` fields are
    /// taken from the [`AuditLog`] as-is.
    async fn insert(&self, entry: &AuditLog) -> Result<(), AuditError>;

    /// List audit log entries matching the given filter, with cursor
    /// pagination by `AuditLogId`.
    ///
    /// Returns up to `limit` entries whose `AuditLogId` is strictly less
    /// than `cursor` (or all entries if `cursor` is `None`), ordered by
    /// `id` descending (newest first). ULIDs are lexically sortable by
    /// creation time, so the cursor is the oldest entry's ID from the
    /// previous page.
    async fn list(
        &self,
        filter: &AuditLogFilter,
        cursor: Option<AuditLogId>,
        limit: u32,
    ) -> Result<Vec<AuditLog>, AuditError>;

    /// Read at most 500 oldest candidates, plus a bounded has-more probe.
    async fn preview_cleanup(&self, before: Timestamp) -> Result<AuditCleanupPreview, AuditError>;

    /// Recheck the exact candidate batch under a write transaction, delete it
    /// and append its receipt atomically. A stale preview returns Conflict.
    async fn cleanup(
        &self,
        before: Timestamp,
        entry_ids: &[AuditLogId],
        receipt: &AuditLog,
    ) -> Result<(), AuditError>;
}
