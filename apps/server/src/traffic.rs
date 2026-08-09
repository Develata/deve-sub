//! Subscription traffic management route handlers (admin-only).
//!
//! Implements the `/api/v1/subscriptions/{id}/traffic` and
//! `/api/v1/subscriptions/{id}/traffic-correction` endpoints: query aggregated
//! traffic and apply manual corrections. All routes require an authenticated
//! admin via the [`AdminUser`] extractor. See
//! `docs/plan/milestones/M6-subscription-distribution.md` §"Traffic and expiry
//! policy framework".

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use deve_sub_application::subscription;
use deve_sub_contract::{
    ErrorResponse, ManualCorrectionRequest, ManualCorrectionResponse, TrafficSourceBreakdownDto,
    TrafficSummaryResponse,
};
use deve_sub_kernel::SubscriptionId;

use crate::AppState;
use crate::auth::{AdminUser, err, ts_to_iso8601};

/// `GET /api/v1/subscriptions/{id}/traffic` — get aggregated traffic summary
/// for a subscription (admin). Returns consumed upload/download totals and a
/// per-source-kind breakdown.
#[utoipa::path(
    get,
    path = "/api/v1/subscriptions/{id}/traffic",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Subscription ULID")),
    responses(
        (status = 200, description = "Traffic summary", body = TrafficSummaryResponse),
        (status = 400, description = "Invalid subscription id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn get_traffic(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<TrafficSummaryResponse>, (StatusCode, Json<ErrorResponse>)> {
    let subscription_id = SubscriptionId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "subscription id is not a valid ULID",
        )
    })?;

    let summary = subscription::get_traffic_summary(state.traffic_repo.as_ref(), subscription_id)
        .await
        .map_err(|e| map_traffic_error(e, "get_traffic"))?;

    Ok(Json(TrafficSummaryResponse {
        subscription_id: id,
        upload: summary.upload,
        download: summary.download,
        total: summary.total(),
        by_source: summary
            .by_source
            .into_iter()
            .map(|(kind, u, d)| TrafficSourceBreakdownDto {
                source_kind: kind.as_kebab().to_owned(),
                upload: u,
                download: d,
            })
            .collect(),
    }))
}

/// `POST /api/v1/subscriptions/{id}/traffic-correction` — apply a manual
/// traffic correction (admin). Appends a `manual-correction` record; aggregation
/// is sum-based.
#[utoipa::path(
    post,
    path = "/api/v1/subscriptions/{id}/traffic-correction",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Subscription ULID")),
    request_body = ManualCorrectionRequest,
    responses(
        (status = 201, description = "Correction applied", body = ManualCorrectionResponse),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn apply_traffic_correction(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<ManualCorrectionRequest>,
) -> Result<(StatusCode, Json<ManualCorrectionResponse>), (StatusCode, Json<ErrorResponse>)> {
    let subscription_id = SubscriptionId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "subscription id is not a valid ULID",
        )
    })?;

    let record = subscription::apply_manual_correction(
        state.traffic_repo.as_ref(),
        subscription::ManualCorrectionParams {
            subscription_id,
            upload: req.upload,
            download: req.download,
            note: req.note,
        },
    )
    .await
    .map_err(|e| map_traffic_error(e, "apply_traffic_correction"))?;

    Ok((
        StatusCode::CREATED,
        Json(ManualCorrectionResponse {
            record_id: record.id.to_string(),
            subscription_id: record.subscription_id.to_string(),
            source_kind: record.source_kind.as_kebab().to_owned(),
            upload: record.upload,
            download: record.download,
            recorded_at: ts_to_iso8601(record.recorded_at),
            source_ref: record.source_ref,
        }),
    ))
}

/// Map a [`subscription::SubscriptionAppError`] to an HTTP error response.
fn map_traffic_error(
    e: deve_sub_application::SubscriptionAppError,
    ctx: &str,
) -> (StatusCode, Json<ErrorResponse>) {
    use deve_sub_application::SubscriptionAppError;
    match e {
        SubscriptionAppError::InvalidInput(msg) => {
            err(StatusCode::BAD_REQUEST, "invalid_input", &msg)
        }
        other => {
            tracing::warn!(error = %other, "{ctx} failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("failed to {ctx}"),
            )
        }
    }
}

/// Register all traffic management routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(get_traffic))
        .routes(routes!(apply_traffic_correction))
}
