//! Non-sensitive audit metadata for node organization commands (M10).

use deve_sub_domain::{AuditLog, AuditLogRepository};
use deve_sub_kernel::UserId;

/// Best-effort record of a successful node/tag operation. Only stable action
/// codes, entity IDs and counts enter audit metadata; node payloads never do.
pub async fn record_node_action(
    repo: &dyn AuditLogRepository,
    actor: UserId,
    action: &'static str,
    target_type: &'static str,
    target_id: Option<String>,
    affected: u64,
) {
    let entry = AuditLog::new(
        Some(actor),
        action,
        Some(target_type.into()),
        target_id,
        Some(serde_json::json!({ "affected": affected }).to_string()),
    );
    if let Err(error) = super::record_audit_log(repo, &entry).await {
        tracing::warn!(%error, action, "audit recording failed after successful mutation");
    }
}
