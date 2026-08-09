//! Probe route handlers (admin-only): probe source CRUD, probe run lifecycle,
//! and latency query.
//!
//! Implements `/api/v1/probe-sources/*`, `/api/v1/probe-runs/*`, and
//! `/api/v1/nodes/{id}/latency`. All routes require an authenticated admin via
//! the [`AdminUser`] extractor. See
//! `docs/plan/milestones/M7-probes-and-detection.md` §"Server".

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use deve_sub_application::probe::{
    self, CreateProbeSourceParams, ProbeAppError, RunnerConfig, StartProbeRunParams,
    UpdateProbeSourceParams, execute_probe_run,
};
use deve_sub_contract::{
    CreateProbeRunRequest, CreateProbeSourceRequest, ErrorClassDto, LatencyRecordDto,
    ListLatencyRecordsResponse, ListProbeSourcesResponse, ProbeRunDto, ProbeRunResponse,
    ProbeRunResultDto, ProbeRunStatusDto, ProbeSourceDto, ProbeSourceKindDto, ProbeSourceResponse,
    ProbeTypeDto, SyncStatusDto, UpdateProbeSourceRequest,
};
use deve_sub_domain::{
    ErrorClass, LatencyRecord, ProbeRun, ProbeRunStatus, ProbeSource,
    ProbeSourceKind, ProbeType, SyncStatus,
};
use deve_sub_kernel::{NodeId, ProbeRunId, ProbeSourceId, SubscriptionId};

use crate::AppState;
use crate::auth::{AdminUser, err, ts_to_iso8601};

/// Query parameters for `GET /api/v1/probe-sources`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ListProbeSourcesQuery {
    #[serde(default = "default_page_size")]
    pub limit: u32,
    pub cursor: Option<String>,
    pub kind: Option<String>,
}

fn default_page_size() -> u32 {
    20
}

/// Query parameters for `GET /api/v1/nodes/{id}/latency`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ListLatencyQuery {
    #[serde(default = "default_latency_limit")]
    pub limit: u32,
}

fn default_latency_limit() -> u32 {
    50
}

fn kind_to_dto(k: ProbeSourceKind) -> ProbeSourceKindDto {
    match k {
        ProbeSourceKind::Nezha => ProbeSourceKindDto::Nezha,
        ProbeSourceKind::DStatus => ProbeSourceKindDto::Dstatus,
        ProbeSourceKind::Komari => ProbeSourceKindDto::Komari,
    }
}

fn kind_from_dto(d: ProbeSourceKindDto) -> ProbeSourceKind {
    match d {
        ProbeSourceKindDto::Nezha => ProbeSourceKind::Nezha,
        ProbeSourceKindDto::Dstatus => ProbeSourceKind::DStatus,
        ProbeSourceKindDto::Komari => ProbeSourceKind::Komari,
    }
}

fn kind_from_kebab(s: &str) -> Option<ProbeSourceKind> {
    match s {
        "nezha" => Some(ProbeSourceKind::Nezha),
        "dstatus" => Some(ProbeSourceKind::DStatus),
        "komari" => Some(ProbeSourceKind::Komari),
        _ => None,
    }
}

fn probe_type_to_dto(t: ProbeType) -> ProbeTypeDto {
    match t {
        ProbeType::TcpConnect => ProbeTypeDto::TcpConnect,
        ProbeType::QuicHandshake => ProbeTypeDto::QuicHandshake,
        ProbeType::RealProxy => ProbeTypeDto::RealProxy,
    }
}

fn probe_type_from_dto(d: ProbeTypeDto) -> ProbeType {
    match d {
        ProbeTypeDto::TcpConnect => ProbeType::TcpConnect,
        ProbeTypeDto::QuicHandshake => ProbeType::QuicHandshake,
        ProbeTypeDto::RealProxy => ProbeType::RealProxy,
    }
}

fn run_status_to_dto(s: ProbeRunStatus) -> ProbeRunStatusDto {
    match s {
        ProbeRunStatus::Pending => ProbeRunStatusDto::Pending,
        ProbeRunStatus::Running => ProbeRunStatusDto::Running,
        ProbeRunStatus::Completed => ProbeRunStatusDto::Completed,
        ProbeRunStatus::Cancelled => ProbeRunStatusDto::Cancelled,
        ProbeRunStatus::Failed => ProbeRunStatusDto::Failed,
    }
}

fn error_class_to_dto(c: ErrorClass) -> ErrorClassDto {
    match c {
        ErrorClass::Refused => ErrorClassDto::Refused,
        ErrorClass::DnsFailed => ErrorClassDto::DnsFailed,
        ErrorClass::Timeout => ErrorClassDto::Timeout,
        ErrorClass::TlsFailed => ErrorClassDto::TlsFailed,
        ErrorClass::QuicFailed => ErrorClassDto::QuicFailed,
        ErrorClass::Ok => ErrorClassDto::Ok,
    }
}

fn sync_status_to_dto(s: &SyncStatus) -> SyncStatusDto {
    match s {
        SyncStatus::Ok => SyncStatusDto::Ok,
        SyncStatus::Failed(msg) => SyncStatusDto::Failed {
            message: msg.clone(),
        },
        SyncStatus::Stale => SyncStatusDto::Stale,
    }
}

fn source_to_dto(source: &ProbeSource) -> ProbeSourceDto {
    ProbeSourceDto {
        id: source.id.to_string(),
        kind: kind_to_dto(source.kind),
        name: source.name.clone(),
        endpoint_url: source.endpoint_url.clone(),
        has_auth: !source.auth_config.is_empty(),
        subscription_id: source.subscription_id.map(|id| id.to_string()),
        enabled: source.enabled,
        last_sync_at: source.last_sync_at.map(ts_to_iso8601),
        last_sync_status: source.last_sync_status.as_ref().map(sync_status_to_dto),
        created_at: ts_to_iso8601(source.created_at),
        updated_at: ts_to_iso8601(source.updated_at),
    }
}

fn run_to_dto(run: &ProbeRun) -> ProbeRunDto {
    ProbeRunDto {
        id: run.id.to_string(),
        probe_type: probe_type_to_dto(run.probe_type),
        node_ids: run.node_ids.iter().map(|id| id.to_string()).collect(),
        status: run_status_to_dto(run.status),
        results: run
            .results
            .iter()
            .map(|r| ProbeRunResultDto {
                node_id: r.node_id.to_string(),
                rtt_ms: r.rtt_ms,
                error_class: error_class_to_dto(r.error_class),
                skipped: r.skipped,
            })
            .collect(),
        created_at: ts_to_iso8601(run.created_at),
        completed_at: run.completed_at.map(ts_to_iso8601),
    }
}

fn record_to_dto(record: &LatencyRecord) -> LatencyRecordDto {
    LatencyRecordDto {
        id: record.id.to_string(),
        run_id: record.run_id.to_string(),
        node_id: record.node_id.to_string(),
        probe_type: probe_type_to_dto(record.probe_type),
        rtt_ms: record.rtt_ms,
        error_class: error_class_to_dto(record.error_class),
        measured_at: ts_to_iso8601(record.measured_at),
    }
}

fn map_probe_error(e: ProbeAppError) -> (StatusCode, Json<deve_sub_contract::ErrorResponse>) {
    match e {
        ProbeAppError::InvalidInput(msg) => err(StatusCode::BAD_REQUEST, "invalid_input", &msg),
        ProbeAppError::SourceNotFound => err(
            StatusCode::NOT_FOUND,
            "source_not_found",
            "probe source does not exist",
        ),
        ProbeAppError::RunNotFound => err(
            StatusCode::NOT_FOUND,
            "run_not_found",
            "probe run does not exist",
        ),
        ProbeAppError::NameExists => err(
            StatusCode::CONFLICT,
            "name_exists",
            "probe source name is already taken",
        ),
        ProbeAppError::RunAlreadyTerminal => err(
            StatusCode::CONFLICT,
            "run_already_terminal",
            "probe run is already completed, cancelled, or failed",
        ),
        ProbeAppError::Domain(deve_sub_domain::ProbeError::NameExists) => err(
            StatusCode::CONFLICT,
            "name_exists",
            "probe source name is already taken",
        ),
        ProbeAppError::Domain(deve_sub_domain::ProbeError::SourceNotFound) => err(
            StatusCode::NOT_FOUND,
            "source_not_found",
            "probe source does not exist",
        ),
        ProbeAppError::Domain(deve_sub_domain::ProbeError::RunNotFound) => err(
            StatusCode::NOT_FOUND,
            "run_not_found",
            "probe run does not exist",
        ),
        other => {
            tracing::warn!(error = %other, "probe operation failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "probe operation failed",
            )
        }
    }
}

/// `POST /api/v1/probe-sources` — create a probe source (admin only).
#[utoipa::path(
    post,
    path = "/api/v1/probe-sources",
    security(("cookie_auth" = [])),
    request_body = CreateProbeSourceRequest,
    responses(
        (status = 201, description = "Probe source created", body = ProbeSourceResponse),
        (status = 400, description = "Invalid input", body = deve_sub_contract::ErrorResponse),
        (status = 401, description = "Not authenticated", body = deve_sub_contract::ErrorResponse),
        (status = 403, description = "Not an admin", body = deve_sub_contract::ErrorResponse),
        (status = 409, description = "Name exists", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn create_probe_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(req): Json<CreateProbeSourceRequest>,
) -> Result<
    (StatusCode, Json<ProbeSourceResponse>),
    (StatusCode, Json<deve_sub_contract::ErrorResponse>),
> {
    let subscription_id = req
        .subscription_id
        .as_deref()
        .map(SubscriptionId::parse)
        .transpose()
        .map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_subscription_id",
                "invalid subscription ULID",
            )
        })?;

    let source = probe::create_probe_source(
        state.probe_source_repo.as_ref(),
        CreateProbeSourceParams {
            kind: kind_from_dto(req.kind),
            name: req.name,
            endpoint_url: req.endpoint_url,
            auth_config: req.auth_config,
            subscription_id,
        },
    )
    .await
    .map_err(map_probe_error)?;

    Ok((
        StatusCode::CREATED,
        Json(ProbeSourceResponse {
            source: source_to_dto(&source),
        }),
    ))
}

/// `GET /api/v1/probe-sources` — list probe sources (admin only).
#[utoipa::path(
    get,
    path = "/api/v1/probe-sources",
    security(("cookie_auth" = [])),
    params(
        ("limit" = Option<u32>, Query, description = "Max sources per page (1-100, default 20)"),
        ("cursor" = Option<String>, Query, description = "Pagination cursor"),
        ("kind" = Option<String>, Query, description = "Filter by kind (nezha/dstatus/komari)"),
    ),
    responses(
        (status = 200, description = "Probe source list", body = ListProbeSourcesResponse),
        (status = 400, description = "Invalid cursor", body = deve_sub_contract::ErrorResponse),
        (status = 401, description = "Not authenticated", body = deve_sub_contract::ErrorResponse),
        (status = 403, description = "Not an admin", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn list_probe_sources(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<ListProbeSourcesQuery>,
) -> Result<Json<ListProbeSourcesResponse>, (StatusCode, Json<deve_sub_contract::ErrorResponse>)> {
    let limit = q.limit.clamp(1, 100);
    let cursor = q
        .cursor
        .as_deref()
        .map(ProbeSourceId::parse)
        .transpose()
        .map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "cursor is not a valid ULID",
            )
        })?;
    let kind = q.kind.as_deref().and_then(kind_from_kebab);

    let sources = probe::list_probe_sources(state.probe_source_repo.as_ref(), cursor, limit, kind)
        .await
        .map_err(map_probe_error)?;

    let next_cursor = if sources.len() as u32 >= limit {
        sources.last().map(|s| s.id.to_string())
    } else {
        None
    };

    Ok(Json(ListProbeSourcesResponse {
        sources: sources.iter().map(source_to_dto).collect(),
        next_cursor,
    }))
}

/// `GET /api/v1/probe-sources/{id}` — get a probe source (admin only).
#[utoipa::path(
    get,
    path = "/api/v1/probe-sources/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Probe source ULID")),
    responses(
        (status = 200, description = "Probe source found", body = ProbeSourceResponse),
        (status = 400, description = "Invalid id", body = deve_sub_contract::ErrorResponse),
        (status = 401, description = "Not authenticated", body = deve_sub_contract::ErrorResponse),
        (status = 403, description = "Not an admin", body = deve_sub_contract::ErrorResponse),
        (status = 404, description = "Not found", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn get_probe_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<ProbeSourceResponse>, (StatusCode, Json<deve_sub_contract::ErrorResponse>)> {
    let source_id = ProbeSourceId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "probe source id is not a valid ULID",
        )
    })?;
    let source = probe::get_probe_source(state.probe_source_repo.as_ref(), source_id)
        .await
        .map_err(map_probe_error)?;
    Ok(Json(ProbeSourceResponse {
        source: source_to_dto(&source),
    }))
}

/// `PUT /api/v1/probe-sources/{id}` — update a probe source (admin only).
#[utoipa::path(
    put,
    path = "/api/v1/probe-sources/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Probe source ULID")),
    request_body = UpdateProbeSourceRequest,
    responses(
        (status = 200, description = "Probe source updated", body = ProbeSourceResponse),
        (status = 400, description = "Invalid input", body = deve_sub_contract::ErrorResponse),
        (status = 401, description = "Not authenticated", body = deve_sub_contract::ErrorResponse),
        (status = 403, description = "Not an admin", body = deve_sub_contract::ErrorResponse),
        (status = 404, description = "Not found", body = deve_sub_contract::ErrorResponse),
        (status = 409, description = "Name exists", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn update_probe_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateProbeSourceRequest>,
) -> Result<Json<ProbeSourceResponse>, (StatusCode, Json<deve_sub_contract::ErrorResponse>)> {
    let source_id = ProbeSourceId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "probe source id is not a valid ULID",
        )
    })?;
    let subscription_id = match req.subscription_id {
        Some(Some(s)) => Some(Some(SubscriptionId::parse(&s).map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_subscription_id",
                "invalid subscription ULID",
            )
        })?)),
        Some(None) => Some(None),
        None => None,
    };

    let source = probe::update_probe_source(
        state.probe_source_repo.as_ref(),
        UpdateProbeSourceParams {
            id: source_id,
            name: req.name,
            endpoint_url: req.endpoint_url,
            auth_config: req.auth_config,
            subscription_id,
            enabled: req.enabled,
        },
    )
    .await
    .map_err(map_probe_error)?;

    Ok(Json(ProbeSourceResponse {
        source: source_to_dto(&source),
    }))
}

/// `DELETE /api/v1/probe-sources/{id}` — delete a probe source (admin only).
#[utoipa::path(
    delete,
    path = "/api/v1/probe-sources/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Probe source ULID")),
    responses(
        (status = 200, description = "Probe source deleted"),
        (status = 400, description = "Invalid id", body = deve_sub_contract::ErrorResponse),
        (status = 401, description = "Not authenticated", body = deve_sub_contract::ErrorResponse),
        (status = 403, description = "Not an admin", body = deve_sub_contract::ErrorResponse),
        (status = 404, description = "Not found", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn delete_probe_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<deve_sub_contract::ErrorResponse>)> {
    let source_id = ProbeSourceId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "probe source id is not a valid ULID",
        )
    })?;
    probe::delete_probe_source(state.probe_source_repo.as_ref(), source_id)
        .await
        .map_err(map_probe_error)?;
    Ok(StatusCode::OK)
}

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
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn create_probe_run(
    State(state): State<AppState>,
    _admin: AdminUser,
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

    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut flags = state.cancelled_flags.lock().map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "lock_poisoned",
                "cancellation flag lock poisoned",
            )
        })?;
        flags.insert(run.id, Arc::clone(&cancelled));
    }

    let probe_adapter = match probe_type {
        ProbeType::TcpConnect => Arc::clone(&state.tcp_probe),
        ProbeType::QuicHandshake | ProbeType::RealProxy => {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "unsupported_probe_type",
                "QUIC and real-proxy probes are not yet available",
            ));
        }
    };
    let deps = deve_sub_application::probe::RunnerDeps {
        probe: probe_adapter,
        pool_repo: Arc::clone(&state.pool_repo),
        run_repo: Arc::clone(&state.probe_run_repo),
        latency_repo: Arc::clone(&state.latency_repo),
    };
    let flags_map = Arc::clone(&state.cancelled_flags);
    let run_id = run.id;

    tokio::spawn(async move {
        execute_probe_run(
            run_id,
            node_ids,
            probe_type,
            deps,
            cancelled,
            RunnerConfig::default(),
        )
        .await
        .ok();
        if let Ok(mut flags) = flags_map.lock() {
            flags.remove(&run_id);
        }
    });

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
    State(state): State<AppState>,
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
    State(state): State<AppState>,
    _admin: AdminUser,
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
    Ok(StatusCode::OK)
}

/// `GET /api/v1/nodes/{id}/latency` — list recent latency records for a node.
#[utoipa::path(
    get,
    path = "/api/v1/nodes/{id}/latency",
    security(("cookie_auth" = [])),
    params(
        ("id" = String, Path, description = "Node ULID"),
        ("limit" = Option<u32>, Query, description = "Max records (1-200, default 50)"),
    ),
    responses(
        (status = 200, description = "Latency records", body = ListLatencyRecordsResponse),
        (status = 400, description = "Invalid id", body = deve_sub_contract::ErrorResponse),
        (status = 401, description = "Not authenticated", body = deve_sub_contract::ErrorResponse),
        (status = 403, description = "Not an admin", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn list_node_latency(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Query(q): Query<ListLatencyQuery>,
) -> Result<Json<ListLatencyRecordsResponse>, (StatusCode, Json<deve_sub_contract::ErrorResponse>)>
{
    let node_id = NodeId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "node id is not a valid ULID",
        )
    })?;
    let limit = q.limit.clamp(1, 200);
    let records = state
        .latency_repo
        .list_for_node(node_id, limit)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_node_latency failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to list latency records",
            )
        })?;
    Ok(Json(ListLatencyRecordsResponse {
        records: records.iter().map(record_to_dto).collect(),
    }))
}

/// Register all probe routes on the given `OpenApiRouter`.
#[allow(clippy::too_many_lines)]
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(create_probe_source))
        .routes(routes!(list_probe_sources))
        .routes(routes!(get_probe_source))
        .routes(routes!(update_probe_source))
        .routes(routes!(delete_probe_source))
        .routes(routes!(create_probe_run))
        .routes(routes!(get_probe_run))
        .routes(routes!(cancel_probe_run))
        .routes(routes!(list_node_latency))
}
