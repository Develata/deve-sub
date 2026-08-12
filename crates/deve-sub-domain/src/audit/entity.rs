//! Audit log entity.

use deve_sub_kernel::{AuditLogId, Timestamp, UserId};
use serde::{Deserialize, Serialize};

/// An append-only record of an actor's action on a target.
///
/// The audit log captures who did what to which entity, with optional
/// non-sensitive metadata. Rows are never updated or deleted (except via a
/// future retention policy). The `actor_id` foreign key uses `ON DELETE SET
/// NULL` so deleting a user preserves their audit history with the actor
/// anonymized.
///
/// See `docs/plan/milestones/M10-observability-and-audit.md` §"Audit log
/// model".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditLog {
    /// Unique identifier (ULID).
    pub id: AuditLogId,
    /// The user who performed the action. `None` for system or anonymous
    /// actions (e.g. initial admin setup).
    pub actor_id: Option<UserId>,
    /// Action string following `{module}.{verb}` convention (e.g.
    /// `"auth.login"`, `"user.create"`).
    pub action: String,
    /// The type of entity acted upon (e.g. `"user"`, `"source"`). `None`
    /// for actions without a specific target.
    pub target_type: Option<String>,
    /// The ID of the target entity. `None` for actions without a specific
    /// target.
    pub target_id: Option<String>,
    /// Non-sensitive metadata as a JSON string. Must NOT contain secrets,
    /// tokens, passwords, or subscription URLs.
    pub details_json: Option<String>,
    /// When the action was recorded.
    pub created_at: Timestamp,
}

impl AuditLog {
    /// Create a new audit log entry with the given fields and a generated
    /// ULID + current timestamp.
    #[must_use]
    pub fn new(
        actor_id: Option<UserId>,
        action: impl Into<String>,
        target_type: Option<String>,
        target_id: Option<String>,
        details_json: Option<String>,
    ) -> Self {
        Self {
            id: AuditLogId::new(),
            actor_id,
            action: action.into(),
            target_type,
            target_id,
            details_json,
            created_at: Timestamp::now(),
        }
    }
}
