//! Bounded audit cleanup with preview and transactional accountability (M10).

use std::collections::HashSet;

use deve_sub_domain::{
    AUDIT_CLEANUP_BATCH, AuditCleanupPreview, AuditError, AuditLog, AuditLogRepository,
};
use deve_sub_kernel::{AuditLogId, Timestamp, UserId};

/// Preview events older than the requested retention; the server owns the clock.
pub async fn preview_cleanup(
    repo: &dyn AuditLogRepository,
    keep_days: u32,
) -> Result<AuditCleanupPreview, AuditError> {
    if !(1..=3650).contains(&keep_days) {
        return Err(AuditError::Invalid(
            "keep_days must be between 1 and 3650".into(),
        ));
    }
    let before = Timestamp::now()
        .checked_sub(time::Duration::days(i64::from(keep_days)))
        .ok_or_else(|| AuditError::Invalid("cutoff is out of range".into()))?;
    // Whole-second UTC precision matches SQLite timestamp storage.
    let before = Timestamp::from_unix_ms(before.unix_ms().div_euclid(1000) * 1000)
        .map_err(|_| AuditError::Invalid("cutoff is out of range".into()))?;
    repo.preview_cleanup(before).await
}

/// Confirm a previewed batch. The receipt is committed with the deletion, so
/// losing the response or retrying cannot silently remove the next batch.
pub async fn cleanup(
    repo: &dyn AuditLogRepository,
    before: Timestamp,
    entry_ids: &[AuditLogId],
    actor: Option<UserId>,
) -> Result<AuditLogId, AuditError> {
    let latest = Timestamp::now()
        .checked_sub(time::Duration::days(1))
        .ok_or_else(|| AuditError::Invalid("cutoff is out of range".into()))?;
    if before.unix_ms().rem_euclid(1000) != 0
        || before > latest
        || entry_ids.is_empty()
        || entry_ids.len() > AUDIT_CLEANUP_BATCH
        || entry_ids.iter().collect::<HashSet<_>>().len() != entry_ids.len()
    {
        return Err(AuditError::Invalid(
            "keep at least one day and confirm 1–500 distinct previewed IDs".into(),
        ));
    }
    let reason = if actor.is_some() {
        "manual"
    } else {
        "retention"
    };
    let receipt = AuditLog::new(actor, "audit.cleanup", Some("audit_log".into()), None,
        Some(serde_json::json!({ "reason": reason, "before_unix_ms": before.unix_ms(), "deleted": entry_ids.len() }).to_string()));
    repo.cleanup(before, entry_ids, &receipt).await?;
    tracing::info!(receipt_id = %receipt.id, deleted = entry_ids.len(), reason, before_unix_ms = before.unix_ms(), "audit cleanup completed");
    Ok(receipt.id)
}

/// Run one automatic batch. Zero disables expiry and never queries storage.
pub async fn prune_audit_logs(
    repo: &dyn AuditLogRepository,
    keep_days: u32,
) -> Result<u64, AuditError> {
    if keep_days == 0 {
        return Ok(0);
    }
    let preview = preview_cleanup(repo, keep_days).await?;
    if preview.entry_ids.is_empty() {
        return Ok(0);
    }
    cleanup(repo, preview.before, &preview.entry_ids, None).await?;
    Ok(preview.entry_ids.len() as u64)
}
