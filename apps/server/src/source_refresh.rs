//! Source refresh job handlers (B-15): async refresh, job status, cancel.
//!
//! The refresh is now asynchronous: `POST /api/v1/sources/{id}/refresh`
//! returns 202 with a job ID. The client polls
//! `GET /api/v1/sources/refresh-jobs/{job_id}` for status and can cancel
//! via `POST /api/v1/sources/refresh-jobs/{job_id}/cancel`.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use deve_sub_application::audit;
use deve_sub_application::source::{
    self, RefreshDeps, execute_refresh_job, signal_cancel, start_refresh_job,
};
use deve_sub_contract::{
    CancelRefreshJobResponse, ErrorResponse, RefreshJobAcceptedResponse, SourceRefreshJobDto,
};
use deve_sub_domain::SourceRefreshJobStatus;
use deve_sub_kernel::{SourceId, SourceRefreshJobId};

use crate::AppState;
use crate::auth::{AdminUser, err, ts_to_iso8601};

/// Shared map of cancel flags for in-flight refresh jobs, keyed by job ID.
/// Stored in `AppState` alongside the probe `cancelled_flags`.
pub type RefreshCancelFlags = Arc<Mutex<HashMap<SourceRefreshJobId, Arc<AtomicBool>>>>;

/// `POST /api/v1/sources/{id}/refresh` — start an async refresh job (B-15).
///
/// Returns 202 Accepted with the job ID. The refresh runs in a background
/// task via `JobSupervisor`. The client polls the job status endpoint.
#[utoipa::path(
    post,
    path = "/api/v1/sources/{id}/refresh",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Source ULID")),
    responses(
        (status = 202, description = "Refresh job accepted", body = RefreshJobAcceptedResponse),
        (status = 400, description = "Invalid source id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 409, description = "Refresh already in progress", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
pub async fn refresh_source(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<RefreshJobAcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    let source_id = SourceId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "source id is not a valid ULID",
        )
    })?;

    let deps = RefreshDeps {
        source_repo: state.source_repo.as_ref(),
        snapshot_repo: state.snapshot_repo.as_ref(),
        pool_repo: state.pool_repo.as_ref(),
        job_repo: state.refresh_job_repo.as_ref(),
        fetcher: state.fetcher.as_ref(),
        geoip: state.geoip.as_ref(),
    };

    let job_id = start_refresh_job(&deps, source_id)
        .await
        .map_err(|e| match e {
            source::SourceAppError::RefreshInProgress(_) => err(
                StatusCode::CONFLICT,
                "refresh_in_progress",
                "a refresh is already in progress for this source",
            ),
            source::SourceAppError::SourceNotFound => err(
                StatusCode::NOT_FOUND,
                "source_not_found",
                "source does not exist",
            ),
            other => {
                tracing::warn!(error = %other, "start_refresh_job failed");
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "failed to start refresh job",
                )
            }
        })?;

    let entry = audit::audit_source_refresh(admin.user.id, &source_id.to_string());
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for source.refresh");
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    if let Ok(mut flags) = state.refresh_cancel_flags.lock() {
        flags.insert(job_id, Arc::clone(&cancelled));
    }

    let state2 = state.clone();
    state.job_supervisor.spawn(async move {
        let deps = RefreshDeps {
            source_repo: state2.source_repo.as_ref(),
            snapshot_repo: state2.snapshot_repo.as_ref(),
            pool_repo: state2.pool_repo.as_ref(),
            job_repo: state2.refresh_job_repo.as_ref(),
            fetcher: state2.fetcher.as_ref(),
            geoip: state2.geoip.as_ref(),
        };
        let _ = execute_refresh_job(&deps, job_id, source_id, &cancelled).await;
        if let Ok(mut flags) = state2.refresh_cancel_flags.lock() {
            flags.remove(&job_id);
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(RefreshJobAcceptedResponse {
            job_id: job_id.to_string(),
            source_id: source_id.to_string(),
            status: SourceRefreshJobStatus::Running.as_kebab().to_owned(),
        }),
    ))
}

/// `GET /api/v1/sources/refresh-jobs/{job_id}` — get refresh job status.
#[utoipa::path(
    get,
    path = "/api/v1/sources/refresh-jobs/{job_id}",
    security(("cookie_auth" = [])),
    params(("job_id" = String, Path, description = "Refresh job ULID")),
    responses(
        (status = 200, description = "Job status", body = SourceRefreshJobDto),
        (status = 400, description = "Invalid job id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Job not found", body = ErrorResponse),
    )
)]
pub async fn get_refresh_job(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(job_id): Path<String>,
) -> Result<Json<SourceRefreshJobDto>, (StatusCode, Json<ErrorResponse>)> {
    let id = SourceRefreshJobId::parse(&job_id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "job id is not a valid ULID",
        )
    })?;

    let job = state
        .refresh_job_repo
        .find_by_id(id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "find_refresh_job failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to query job status",
            )
        })?
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "job_not_found",
                "refresh job does not exist",
            )
        })?;

    Ok(Json(job_to_dto(&job)))
}

/// `GET /api/v1/sources/{id}/refresh-jobs/latest` — get the latest refresh
/// job for a source.
#[utoipa::path(
    get,
    path = "/api/v1/sources/{id}/refresh-jobs/latest",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Source ULID")),
    responses(
        (status = 200, description = "Latest job status", body = SourceRefreshJobDto),
        (status = 400, description = "Invalid source id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "No jobs found", body = ErrorResponse),
    )
)]
pub async fn get_latest_refresh_job(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<SourceRefreshJobDto>, (StatusCode, Json<ErrorResponse>)> {
    let source_id = SourceId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "source id is not a valid ULID",
        )
    })?;

    let jobs = state
        .refresh_job_repo
        .list_for_source(source_id, 1)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_refresh_jobs failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to query job history",
            )
        })?;

    let job = jobs.into_iter().next().ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            "no_jobs",
            "no refresh jobs found for this source",
        )
    })?;

    Ok(Json(job_to_dto(&job)))
}

/// `POST /api/v1/sources/refresh-jobs/{job_id}/cancel` — cancel a refresh
/// job. Sets the cancel flag so the runner aborts at the next phase boundary.
#[utoipa::path(
    post,
    path = "/api/v1/sources/refresh-jobs/{job_id}/cancel",
    security(("cookie_auth" = [])),
    params(("job_id" = String, Path, description = "Refresh job ULID")),
    responses(
        (status = 200, description = "Cancel signal sent", body = CancelRefreshJobResponse),
        (status = 400, description = "Invalid job id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Job not found", body = ErrorResponse),
    )
)]
pub async fn cancel_refresh_job(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(job_id): Path<String>,
) -> Result<Json<CancelRefreshJobResponse>, (StatusCode, Json<ErrorResponse>)> {
    let id = SourceRefreshJobId::parse(&job_id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "job id is not a valid ULID",
        )
    })?;

    let job = state
        .refresh_job_repo
        .find_by_id(id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "find_refresh_job failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to query job status",
            )
        })?
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "job_not_found",
                "refresh job does not exist",
            )
        })?;

    if job.status.is_terminal() {
        return Ok(Json(CancelRefreshJobResponse {
            job_id: id.to_string(),
            cancelled: false,
        }));
    }

    let cancelled = if let Ok(flags) = state.refresh_cancel_flags.lock() {
        flags.get(&id).cloned()
    } else {
        None
    };

    if let Some(flag) = cancelled {
        signal_cancel(&flag);
    } else {
        let _ = state.refresh_job_repo.mark_cancelled(id).await;
    }

    Ok(Json(CancelRefreshJobResponse {
        job_id: id.to_string(),
        cancelled: true,
    }))
}

fn job_to_dto(job: &deve_sub_domain::SourceRefreshJob) -> SourceRefreshJobDto {
    SourceRefreshJobDto {
        id: job.id.to_string(),
        source_id: job.source_id.to_string(),
        status: job.status.as_kebab().to_owned(),
        phase: job.phase.as_db_str().to_owned(),
        started_at: ts_to_iso8601(job.started_at),
        finished_at: job.finished_at.map(ts_to_iso8601),
        error_message: job.error_message.clone(),
        new_nodes: job.new_nodes,
        duplicate_nodes: job.duplicate_nodes,
        reactivated_nodes: job.reactivated_nodes,
        missing_nodes: job.missing_nodes,
        not_modified: job.not_modified,
    }
}

/// Register all source refresh job routes.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(refresh_source))
        .routes(routes!(get_refresh_job))
        .routes(routes!(get_latest_refresh_job))
        .routes(routes!(cancel_refresh_job))
}
