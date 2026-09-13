//! Admin-only audit retention policy, preview, and confirmed cleanup delivery.

use std::future::Future;
use std::time::Duration;

use axum::{Json, extract::State, http::StatusCode};
use deve_sub_application::audit;
use deve_sub_contract::{
    AuditCleanupPreviewRequest, AuditCleanupPreviewResponse, AuditCleanupRequest,
    AuditCleanupResponse, AuditPolicyResponse, ErrorResponse,
};
use deve_sub_domain::{AUDIT_CLEANUP_BATCH, AuditError};
use deve_sub_kernel::{AuditLogId, Timestamp};

use crate::{
    AppState,
    auth::{AdminUser, err},
    state::AuditState,
};

type ApiError = (StatusCode, Json<ErrorResponse>);

#[utoipa::path(get, path = "/api/v1/audit-logs/policy", security(("cookie_auth" = [])),
    responses((status = 200, body = AuditPolicyResponse), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse)))]
async fn policy(State(state): State<AuditState>, _admin: AdminUser) -> Json<AuditPolicyResponse> {
    Json(AuditPolicyResponse {
        retention_days: state.retention_days,
        batch_limit: AUDIT_CLEANUP_BATCH,
    })
}

#[utoipa::path(post, path = "/api/v1/audit-logs/cleanup/preview", security(("cookie_auth" = [])),
    request_body = AuditCleanupPreviewRequest,
    responses((status = 200, body = AuditCleanupPreviewResponse), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse), (status = 500, body = ErrorResponse), (status = 503, body = ErrorResponse)))]
async fn preview(
    State(state): State<AuditState>,
    _admin: AdminUser,
    Json(request): Json<AuditCleanupPreviewRequest>,
) -> Result<Json<AuditCleanupPreviewResponse>, ApiError> {
    let result = bounded(audit::preview_cleanup(
        state.audit_log_repo.as_ref(),
        request.keep_days,
    ))
    .await?;
    Ok(Json(AuditCleanupPreviewResponse {
        before_unix_ms: result.before.unix_ms(),
        entry_ids: result.entry_ids.iter().map(ToString::to_string).collect(),
        has_more: result.has_more,
    }))
}

#[utoipa::path(post, path = "/api/v1/audit-logs/cleanup", security(("cookie_auth" = [])),
    request_body = AuditCleanupRequest,
    responses((status = 200, body = AuditCleanupResponse), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 409, body = ErrorResponse), (status = 500, body = ErrorResponse), (status = 503, body = ErrorResponse)))]
async fn cleanup(
    State(state): State<AuditState>,
    admin: AdminUser,
    Json(request): Json<AuditCleanupRequest>,
) -> Result<Json<AuditCleanupResponse>, ApiError> {
    let before = Timestamp::from_unix_ms(request.before_unix_ms)
        .map_err(|_| map_error(AuditError::Invalid("invalid cutoff".into())))?;
    if request.entry_ids.len() > AUDIT_CLEANUP_BATCH {
        return Err(map_error(AuditError::Invalid(
            "batch exceeds 500 entries".into(),
        )));
    }
    let ids = request
        .entry_ids
        .iter()
        .map(|id| AuditLogId::parse(id))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| map_error(AuditError::Invalid("invalid audit entry ID".into())))?;
    let receipt = bounded(audit::cleanup(
        state.audit_log_repo.as_ref(),
        before,
        &ids,
        Some(admin.user.id),
    ))
    .await?;
    Ok(Json(AuditCleanupResponse {
        deleted: ids.len(),
        receipt_id: receipt.to_string(),
    }))
}

async fn bounded<T>(operation: impl Future<Output = Result<T, AuditError>>) -> Result<T, ApiError> {
    tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .map_err(|_| {
            tracing::warn!("audit operation timed out; query receipts before retrying cleanup");
            err(
                StatusCode::SERVICE_UNAVAILABLE,
                "audit_timeout",
                "operation timed out; refresh audit history before retrying",
            )
        })?
        .map_err(map_error)
}

pub(crate) fn map_error(error: AuditError) -> ApiError {
    match error {
        AuditError::Invalid(message) => {
            err(StatusCode::BAD_REQUEST, "invalid_audit_request", &message)
        }
        AuditError::Conflict => err(
            StatusCode::CONFLICT,
            "audit_preview_stale",
            "audit history changed; preview again",
        ),
        AuditError::Storage(error) => {
            tracing::warn!(%error, "audit operation failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "audit operation failed",
            )
        }
    }
}

pub(crate) fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(policy))
        .routes(routes!(preview))
        .routes(routes!(cleanup))
}
