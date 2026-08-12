//! Audit log route handlers (admin-only).
//!
//! Implements `GET /api/v1/audit-logs` — paginated audit log query with
//! filters (actor, action, target_type, target_id). Admin-only per the
//! M10 blueprint. See AUDIT-001.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use deve_sub_application::audit;
use deve_sub_contract::{AuditLogDto, ErrorResponse, ListAuditLogsResponse};
use deve_sub_domain::AuditLogFilter;
use deve_sub_kernel::{AuditLogId, UserId};

use crate::AppState;
use crate::auth::{AdminUser, err, ts_to_iso8601};

/// Query parameters for `GET /api/v1/audit-logs`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ListAuditLogsQuery {
    /// Maximum number of entries to return (1-100, default 50).
    #[serde(default = "default_page_size")]
    pub limit: u32,
    /// Pagination cursor — the ULID of the oldest entry from the previous
    /// page.
    pub cursor: Option<String>,
    /// Filter by actor ULID.
    pub actor_id: Option<String>,
    /// Filter by action string (e.g. `"auth.login"`).
    pub action: Option<String>,
    /// Filter by target type (e.g. `"user"`).
    pub target_type: Option<String>,
    /// Filter by target ID.
    pub target_id: Option<String>,
}

fn default_page_size() -> u32 {
    50
}

fn entry_to_dto(entry: &deve_sub_domain::AuditLog) -> AuditLogDto {
    AuditLogDto {
        id: entry.id.to_string(),
        actor_id: entry.actor_id.as_ref().map(|id| id.to_string()),
        action: entry.action.clone(),
        target_type: entry.target_type.clone(),
        target_id: entry.target_id.clone(),
        details_json: entry.details_json.clone(),
        created_at: ts_to_iso8601(entry.created_at),
    }
}

/// `GET /api/v1/audit-logs` — list audit log entries with filters and
/// cursor pagination (admin only). AUDIT-001.
#[utoipa::path(
    get,
    path = "/api/v1/audit-logs",
    security(("cookie_auth" = [])),
    params(
        ("limit" = Option<u32>, Query, description = "Max entries per page (1-100, default 50)"),
        ("cursor" = Option<String>, Query, description = "Pagination cursor (oldest entry ULID from previous page)"),
        ("actor_id" = Option<String>, Query, description = "Filter by actor ULID"),
        ("action" = Option<String>, Query, description = "Filter by action (e.g. \"auth.login\")"),
        ("target_type" = Option<String>, Query, description = "Filter by target type (e.g. \"user\")"),
        ("target_id" = Option<String>, Query, description = "Filter by target ID"),
    ),
    responses(
        (status = 200, description = "Audit log entries", body = ListAuditLogsResponse),
        (status = 400, description = "Invalid cursor or actor_id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn list_audit_logs(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<ListAuditLogsQuery>,
) -> Result<Json<ListAuditLogsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = q.limit.clamp(1, 100);

    let cursor = q
        .cursor
        .as_deref()
        .map(AuditLogId::parse)
        .transpose()
        .map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "cursor is not a valid ULID",
            )
        })?;

    let actor_id = q
        .actor_id
        .as_deref()
        .map(UserId::parse)
        .transpose()
        .map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_actor_id",
                "actor_id is not a valid ULID",
            )
        })?;

    let filter = AuditLogFilter {
        actor_id,
        action: q.action,
        target_type: q.target_type,
        target_id: q.target_id,
    };

    let entries = audit::list_audit_logs(state.audit_log_repo.as_ref(), &filter, cursor, limit)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_audit_logs failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to list audit logs",
            )
        })?;

    let next_cursor = if entries.len() as u32 >= limit {
        entries.last().map(|e| e.id.to_string())
    } else {
        None
    };

    let dtos: Vec<AuditLogDto> = entries.iter().map(entry_to_dto).collect();
    Ok(Json(ListAuditLogsResponse {
        entries: dtos,
        next_cursor,
    }))
}

/// Register all audit log routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router.routes(routes!(list_audit_logs))
}
