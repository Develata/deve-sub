//! Port traits for audit log storage.
//!
//! The audit log is append-only: the repository supports insert and filtered
//! list with cursor pagination, but no update or delete. See
//! `docs/plan/milestones/M10-observability-and-audit.md` §"Audit log model".

use async_trait::async_trait;

use deve_sub_kernel::{AuditLogId, UserId};

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
}
