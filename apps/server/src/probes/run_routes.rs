//! Probe-run admission, status and cancellation routes.
use super::{
    error::map_probe_error,
    mappers::{probe_type_from_dto, run_to_dto},
};
use crate::{
    auth::{AdminUser, err},
    state::ProbeState,
};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use deve_sub_application::{
    audit,
    probe::{self, RunnerConfig, StartProbeRunParams, execute_probe_run},
};
use deve_sub_contract::{CreateProbeRunRequest, ProbeRunResponse};
use deve_sub_domain::ProbeType;
use deve_sub_kernel::{NodeId, ProbeRunId};
use std::collections::HashMap;
use std::sync::{Arc, atomic::AtomicBool};

/// `POST /api/v1/probe-runs` — start a probe run (admin only).
///
/// Creates a `Pending` run and spawns the runner as a background task. The
/// response returns immediately with the run in `Pending` status; poll
/// `GET /api/v1/probe-runs/{id}` for progress.
#[utoipa::path(
    post,
    path = "/api/v1/probe-runs",
    security(("cookie_auth" = [])),
    request_body = CreateProbeRunRequest,
    responses(
        (status = 201, description = "Probe run created", body = ProbeRunResponse),
        (status = 400, description = "Invalid input", body = deve_sub_contract::ErrorResponse),
        (status = 401, description = "Not authenticated", body = deve_sub_contract::ErrorResponse),
        (status = 403, description = "Not an admin", body = deve_sub_contract::ErrorResponse),
        (status = 503, description = "Background jobs unavailable", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn create_probe_run(
    State(state): State<ProbeState>,
    admin: AdminUser,
    Json(req): Json<CreateProbeRunRequest>,
) -> Result<
    (StatusCode, Json<ProbeRunResponse>),
    (StatusCode, Json<deve_sub_contract::ErrorResponse>),
> {
    let probe_type = probe_type_from_dto(req.probe_type);
    let node_ids: Vec<NodeId> = req
        .node_ids
        .iter()
        .map(|s| NodeId::parse(s.as_str()))
        .collect::<Result<_, _>>()
        .map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_node_id",
                "one or more node IDs are not valid ULIDs",
            )
        })?;

    let run = probe::start_probe_run(
        state.probe_run_repo.as_ref(),
        StartProbeRunParams {
            probe_type,
            node_ids: node_ids.clone(),
        },
    )
    .await
    .map_err(map_probe_error)?;

    let entry = audit::audit_probe_run_start(admin.user.id, &run.id.to_string());
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for probe.run.start");
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    let registration = deve_sub_application::CancellationRegistration::new(
        Arc::clone(&state.cancelled_flags),
        run.id,
        Arc::clone(&cancelled),
    );

    let probe_adapter = match probe_type {
        ProbeType::TcpConnect => Arc::clone(&state.tcp_probe),
        ProbeType::QuicHandshake => Arc::clone(&state.quic_probe),
        ProbeType::RealProxy => Arc::clone(&state.real_proxy_probe),
    };
    let deps = deve_sub_application::probe::RunnerDeps {
        probe: probe_adapter,
        pool_repo: Arc::clone(&state.pool_repo),
        run_repo: Arc::clone(&state.probe_run_repo),
        latency_repo: Arc::clone(&state.latency_repo),
    };
    let run_id = run.id;

    let supervisor = Arc::clone(&state.job_supervisor);
    let admitted = supervisor.spawn(async move {
        let _registration = registration;
        if let Err(e) = execute_probe_run(
            run_id,
            node_ids,
            probe_type,
            deps,
            cancelled,
            RunnerConfig::default(),
        )
        .await
        {
            tracing::error!(error = %e, %run_id, "probe run failed");
        }
    });
    if admitted.is_err() {
        probe::cancel_probe_run(state.probe_run_repo.as_ref(), &HashMap::new(), run_id)
            .await
            .map_err(map_probe_error)?;
        return Err(err(
            StatusCode::SERVICE_UNAVAILABLE,
            "jobs_unavailable",
            "background jobs unavailable",
        ));
    }

    Ok((
        StatusCode::CREATED,
        Json(ProbeRunResponse {
            run: run_to_dto(&run),
        }),
    ))
}

/// `GET /api/v1/probe-runs/{id}` — get a probe run (admin only).
#[utoipa::path(
    get,
    path = "/api/v1/probe-runs/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Probe run ULID")),
    responses(
        (status = 200, description = "Probe run found", body = ProbeRunResponse),
        (status = 400, description = "Invalid id", body = deve_sub_contract::ErrorResponse),
        (status = 401, description = "Not authenticated", body = deve_sub_contract::ErrorResponse),
        (status = 403, description = "Not an admin", body = deve_sub_contract::ErrorResponse),
        (status = 404, description = "Not found", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn get_probe_run(
    State(state): State<ProbeState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<ProbeRunResponse>, (StatusCode, Json<deve_sub_contract::ErrorResponse>)> {
    let run_id = ProbeRunId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "probe run id is not a valid ULID",
        )
    })?;
    let run = probe::get_probe_run(state.probe_run_repo.as_ref(), run_id)
        .await
        .map_err(map_probe_error)?;
    Ok(Json(ProbeRunResponse {
        run: run_to_dto(&run),
    }))
}

/// `POST /api/v1/probe-runs/{id}/cancel` — cancel a probe run (admin only).
#[utoipa::path(
    post,
    path = "/api/v1/probe-runs/{id}/cancel",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Probe run ULID")),
    responses(
        (status = 200, description = "Probe run cancelled"),
        (status = 400, description = "Invalid id", body = deve_sub_contract::ErrorResponse),
        (status = 401, description = "Not authenticated", body = deve_sub_contract::ErrorResponse),
        (status = 403, description = "Not an admin", body = deve_sub_contract::ErrorResponse),
        (status = 404, description = "Not found", body = deve_sub_contract::ErrorResponse),
        (status = 409, description = "Run already terminal", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn cancel_probe_run(
    State(state): State<ProbeState>,
    admin: AdminUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<deve_sub_contract::ErrorResponse>)> {
    let run_id = ProbeRunId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "probe run id is not a valid ULID",
        )
    })?;
    let flags: HashMap<ProbeRunId, Arc<AtomicBool>> = state
        .cancelled_flags
        .lock()
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "lock_poisoned",
                "cancellation flag lock poisoned",
            )
        })?
        .clone();
    probe::cancel_probe_run(state.probe_run_repo.as_ref(), &flags, run_id)
        .await
        .map_err(map_probe_error)?;

    let entry = audit::audit_probe_run_cancel(admin.user.id, &run_id.to_string());
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for probe.run.cancel");
    }

    Ok(StatusCode::OK)
}

pub(super) fn register(
    router: utoipa_axum::router::OpenApiRouter<crate::AppState>,
) -> utoipa_axum::router::OpenApiRouter<crate::AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(create_probe_run))
        .routes(routes!(get_probe_run))
        .routes(routes!(cancel_probe_run))
}
