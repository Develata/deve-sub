//! Audit log DTOs for the `/api/v1/audit-logs` endpoint.
//!
//! These DTOs are the wire format for audit log queries. Owned by the
//! contract crate per ADR-0004.

use serde::{Deserialize, Serialize};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// A single audit log entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ListAuditLogsResponse {
    /// Audit log entries in the current page (newest first).
    pub entries: Vec<AuditLogDto>,
    /// Cursor for the next page (`None` if no more results). The cursor is
    /// the oldest entry's ULID in the current page.
    pub next_cursor: Option<String>,
}

/// Effective server-owned retention policy; zero disables automatic expiry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct AuditPolicyResponse {
    pub retention_days: u32,
    pub batch_limit: usize,
}

/// Select a cleanup cutoff using the server clock.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct AuditCleanupPreviewRequest {
    /// Whole days to keep, between 1 and 3650.
    pub keep_days: u32,
}

/// Exact bounded snapshot that must be confirmed before deletion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct AuditCleanupPreviewResponse {
    pub before_unix_ms: i64,
    pub entry_ids: Vec<String>,
    pub has_more: bool,
}

/// Confirm exactly the IDs and cutoff returned by preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct AuditCleanupRequest {
    pub before_unix_ms: i64,
    pub entry_ids: Vec<String>,
}

/// Committed cleanup result; the receipt is queryable in the audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct AuditCleanupResponse {
    pub deleted: usize,
    pub receipt_id: String,
}
