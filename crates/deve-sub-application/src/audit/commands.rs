//! Audit log application commands and queries.
//!
//! `record_audit_log` is called after a successful mutation to append an
//! entry. `list_audit_logs` queries entries with filters and cursor
//! pagination. See `docs/plan/milestones/M10-observability-and-audit.md`.

use deve_sub_domain::{AuditError, AuditLog, AuditLogFilter, AuditLogRepository};
use deve_sub_kernel::{AuditLogId, UserId};

/// Append a new audit log entry.
///
/// This is the single application command for audit log writing. Callers
/// pass the already-constructed [`AuditLog`] domain object. The repository
/// insert is not best-effort at this layer — the caller decides whether to
/// ignore the error (the server layer uses `let _ =` for best-effort
/// recording per the M10 blueprint).
///
/// # Errors
/// - [`AuditError::Storage`] — storage failure.
pub async fn record_audit_log(
    repo: &dyn AuditLogRepository,
    entry: &AuditLog,
) -> Result<(), AuditError> {
    repo.insert(entry).await
}

/// List audit log entries with filters and cursor pagination.
///
/// Returns up to `limit` entries matching `filter`, ordered newest-first.
/// The cursor is the oldest entry's `AuditLogId` from the previous page.
///
/// # Errors
/// - [`AuditError::Storage`] — storage failure.
pub async fn list_audit_logs(
    repo: &dyn AuditLogRepository,
    filter: &AuditLogFilter,
    cursor: Option<AuditLogId>,
    limit: u32,
) -> Result<Vec<AuditLog>, AuditError> {
    repo.list(filter, cursor, limit).await
}

/// Convenience builder for a login audit entry.
#[must_use]
pub fn audit_login(actor_id: UserId, success: bool) -> AuditLog {
    AuditLog::new(
        Some(actor_id),
        "auth.login",
        None,
        None,
        Some(serde_json::json!({ "success": success }).to_string()),
    )
}

/// Convenience builder for a logout audit entry.
#[must_use]
pub fn audit_logout(actor_id: UserId) -> AuditLog {
    AuditLog::new(Some(actor_id), "auth.logout", None, None, None)
}

/// Convenience builder for a user-create audit entry.
#[must_use]
pub fn audit_user_create(actor_id: UserId, target_id: &str, username: &str) -> AuditLog {
    AuditLog::new(
        Some(actor_id),
        "user.create",
        Some("user".to_owned()),
        Some(target_id.to_owned()),
        Some(serde_json::json!({ "username": username }).to_string()),
    )
}

/// Convenience builder for a user-disable audit entry.
#[must_use]
pub fn audit_user_disable(actor_id: UserId, target_id: &str) -> AuditLog {
    AuditLog::new(
        Some(actor_id),
        "user.disable",
        Some("user".to_owned()),
        Some(target_id.to_owned()),
        None,
    )
}

/// Convenience builder for a force-logout audit entry.
#[must_use]
pub fn audit_force_logout(actor_id: UserId, target_id: &str) -> AuditLog {
    AuditLog::new(
        Some(actor_id),
        "user.force_logout",
        Some("user".to_owned()),
        Some(target_id.to_owned()),
        None,
    )
}

/// Convenience builder for a 2FA-enable audit entry.
#[must_use]
pub fn audit_2fa_enable(actor_id: UserId) -> AuditLog {
    AuditLog::new(
        Some(actor_id),
        "auth.2fa.enable",
        Some("user".to_owned()),
        Some(actor_id.to_string()),
        None,
    )
}

/// Convenience builder for a 2FA-disable audit entry.
#[must_use]
pub fn audit_2fa_disable(actor_id: UserId) -> AuditLog {
    AuditLog::new(
        Some(actor_id),
        "auth.2fa.disable",
        Some("user".to_owned()),
        Some(actor_id.to_string()),
        None,
    )
}
