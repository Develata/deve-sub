//! Audit log DTOs for the `/api/v1/audit-logs` endpoint.
//!
//! These DTOs are the wire format for audit log queries. Owned by the
//! contract crate per ADR-0004.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AuditLogDto {
    /// ULID identifier.
    pub id: String,
    /// The actor's ULID, or `null` for system/anonymous actions.
    pub actor_id: Option<String>,
    /// Action string (e.g. `"auth.login"`, `"user.create"`).
    pub action: String,
    /// Target entity type (e.g. `"user"`, `"source"`).
    pub target_type: Option<String>,
    /// Target entity ULID.
    pub target_id: Option<String>,
    /// Non-sensitive metadata as a JSON string.
    pub details_json: Option<String>,
    /// When the action was recorded (ISO 8601 UTC).
    pub created_at: String,
}

/// Response body for `GET /api/v1/audit-logs` (cursor-paginated audit log).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListAuditLogsResponse {
    /// Audit log entries in the current page (newest first).
    pub entries: Vec<AuditLogDto>,
    /// Cursor for the next page (`None` if no more results). The cursor is
    /// the oldest entry's ULID in the current page.
    pub next_cursor: Option<String>,
}
