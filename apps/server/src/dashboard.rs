//! Dashboard route handlers (admin-only): aggregated latency and traffic views.
//!
//! Implements `/api/v1/dashboard/latency` and `/api/v1/dashboard/traffic`.
//! All routes require an authenticated admin via the [`AdminUser`] extractor.
//! See `docs/plan/milestones/M7-probes-and-detection.md` §"Traffic aggregation".

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use deve_sub_application::probe::{ProbeAppError, build_dashboard_traffic, list_recent_latency};
use deve_sub_application::{list_traffic_history_for_subscription, list_traffic_history_global};
use deve_sub_contract::{
    DashboardLatencyQuery, DashboardLatencyRecordDto, DashboardLatencyResponse,
    DashboardProbeSourceBreakdownDto, DashboardSourceKindBreakdownDto, DashboardTrafficQuery,
    DashboardTrafficResponse, ErrorResponse, TrafficHistoryPointDto, TrafficHistoryQuery,
    TrafficHistoryResponse, TrafficHistorySourceBreakdownDto,
};
use deve_sub_kernel::SubscriptionId;

use crate::AppState;
use crate::auth::{AdminUser, err, ts_to_iso8601};
use crate::probes::{error_class_to_dto, kind_to_dto, probe_type_to_dto, sync_status_to_dto};

/// `GET /api/v1/dashboard/latency` — recent latency records across all nodes
/// (admin). Returns the most recent records, newest first.
#[utoipa::path(
    get,
    path = "/api/v1/dashboard/latency",
    security(("cookie_auth" = [])),
    params(DashboardLatencyQuery),
    responses(
        (status = 200, description = "Recent latency records", body = DashboardLatencyResponse),
        (status = 400, description = "Invalid limit", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn get_dashboard_latency(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<DashboardLatencyQuery>,
) -> Result<Json<DashboardLatencyResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let records = list_recent_latency(state.latency_repo.as_ref(), limit)
        .await
        .map_err(map_dashboard_error)?;

    let dtos = records
        .into_iter()
        .map(|r| DashboardLatencyRecordDto {
            node_id: r.node_id.to_string(),
            probe_type: probe_type_to_dto(r.probe_type),
            rtt_ms: r.rtt_ms,
            error_class: error_class_to_dto(r.error_class),
            measured_at: ts_to_iso8601(r.measured_at),
        })
        .collect();

    Ok(Json(DashboardLatencyResponse { records: dtos }))
}

/// `GET /api/v1/dashboard/traffic` — global traffic aggregate with per-source
/// and per-probe-source breakdown (admin). Surfaces probe source staleness
/// (PROBE-004) and traceable data provenance (PROBE-005).
#[utoipa::path(
    get,
    path = "/api/v1/dashboard/traffic",
    security(("cookie_auth" = [])),
    params(DashboardTrafficQuery),
    responses(
        (status = 200, description = "Traffic aggregate", body = DashboardTrafficResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn get_dashboard_traffic(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(_q): Query<DashboardTrafficQuery>,
) -> Result<Json<DashboardTrafficResponse>, (StatusCode, Json<ErrorResponse>)> {
    let aggregate = build_dashboard_traffic(
        state.traffic_repo.as_ref(),
        state.probe_source_repo.as_ref(),
    )
    .await
    .map_err(map_dashboard_error)?;

    let by_source_kind = aggregate
        .summary
        .by_source
        .into_iter()
        .map(|(kind, u, d)| DashboardSourceKindBreakdownDto {
            source_kind: kind.as_kebab().to_owned(),
            upload: u,
            download: d,
        })
        .collect();

    let by_probe_source = aggregate
        .by_probe_source
        .into_iter()
        .map(|c| DashboardProbeSourceBreakdownDto {
            source_id: c.source_id.to_string(),
            kind: kind_to_dto(c.kind),
            name: c.name,
            enabled: c.enabled,
            upload: c.upload,
            download: c.download,
            last_sync_at: c.last_sync_at.map(ts_to_iso8601),
            last_sync_status: c.last_sync_status.as_ref().map(sync_status_to_dto),
        })
        .collect();

    Ok(Json(DashboardTrafficResponse {
        total_upload: aggregate.summary.upload,
        total_download: aggregate.summary.download,
        by_source_kind,
        by_probe_source,
    }))
}

#[allow(clippy::needless_pass_by_value)]
fn map_dashboard_error(e: ProbeAppError) -> (StatusCode, Json<ErrorResponse>) {
    let (code, kind, msg) = match &e {
        ProbeAppError::InvalidInput(m) => (StatusCode::BAD_REQUEST, "invalid_input", m.clone()),
        other => {
            tracing::warn!(error = %other, "dashboard query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to build dashboard".to_owned(),
            )
        }
    };
    err(code, kind, &msg)
}

/// `GET /api/v1/dashboard/traffic/history` — daily traffic history (admin).
///
/// Returns daily traffic data points for the last `days` days (default 30).
/// When `subscription_id` is provided, history is scoped to that subscription;
/// otherwise points aggregate across all subscriptions. Days with no traffic
/// are filled with zero-value entries so the chart is continuous (TRAFFIC-002).
#[utoipa::path(
    get,
    path = "/api/v1/dashboard/traffic/history",
    security(("cookie_auth" = [])),
    params(TrafficHistoryQuery),
    responses(
        (status = 200, description = "Daily traffic history", body = TrafficHistoryResponse),
        (status = 400, description = "Invalid query parameters", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn get_traffic_history(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<TrafficHistoryQuery>,
) -> Result<Json<TrafficHistoryResponse>, (StatusCode, Json<ErrorResponse>)> {
    let days = q.days.unwrap_or(30).clamp(1, 365);
    let (start_date, end_date) = compute_date_range(days);

    let (points, scoped) = match &q.subscription_id {
        Some(sub_id_str) => {
            let sub_id = SubscriptionId::parse(sub_id_str).map_err(|_| {
                err(
                    StatusCode::BAD_REQUEST,
                    "invalid_id",
                    "subscription id is not a valid ULID",
                )
            })?;
            let pts = list_traffic_history_for_subscription(
                state.traffic_daily_snapshot_repo.as_ref(),
                sub_id,
                &start_date,
                &end_date,
            )
            .await
            .map_err(map_history_error)?;
            (pts, true)
        }
        None => {
            let pts = list_traffic_history_global(
                state.traffic_daily_snapshot_repo.as_ref(),
                &start_date,
                &end_date,
            )
            .await
            .map_err(map_history_error)?;
            (pts, false)
        }
    };

    let point_dtos = points
        .into_iter()
        .map(|p| TrafficHistoryPointDto {
            date: p.date,
            total_upload: p.total_upload,
            total_download: p.total_download,
            source_breakdown: p
                .source_breakdown
                .into_iter()
                .map(|(kind, u, d)| TrafficHistorySourceBreakdownDto {
                    source_kind: kind.as_kebab().to_owned(),
                    upload: u,
                    download: d,
                })
                .collect(),
        })
        .collect();

    Ok(Json(TrafficHistoryResponse {
        scoped_to_subscription: scoped,
        points: point_dtos,
    }))
}

fn compute_date_range(days: u32) -> (String, String) {
    let now = time::OffsetDateTime::now_utc();
    let end = now.date();
    let start = end - time::Duration::days(i64::from(days) - 1);
    (
        format!(
            "{:04}-{:02}-{:02}",
            start.year(),
            start.month() as u8,
            start.day()
        ),
        format!(
            "{:04}-{:02}-{:02}",
            end.year(),
            end.month() as u8,
            end.day()
        ),
    )
}

#[allow(clippy::needless_pass_by_value)]
fn map_history_error(e: deve_sub_domain::SubscriptionError) -> (StatusCode, Json<ErrorResponse>) {
    use deve_sub_domain::SubscriptionError;
    match e {
        SubscriptionError::Storage(msg) => {
            tracing::warn!(error = %msg, "traffic history storage error");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to query traffic history",
            )
        }
        other => {
            tracing::warn!(error = %other, "traffic history query failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to query traffic history",
            )
        }
    }
}

/// Register all dashboard routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(get_dashboard_latency))
        .routes(routes!(get_dashboard_traffic))
        .routes(routes!(get_traffic_history))
}
