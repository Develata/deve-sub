//! External probe source configuration and traffic synchronization routes.
use super::{
    error::map_probe_error,
    mappers::{kind_from_dto, kind_from_kebab, source_to_dto},
};
use crate::{
    auth::{AdminUser, err},
    state::ProbeState,
};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use deve_sub_application::{
    audit,
    probe::{
        self, CreateProbeSourceParams, ProbeAppError, SyncProbeTrafficResult,
        UpdateProbeSourceParams,
    },
};
use deve_sub_contract::{
    CreateProbeSourceRequest, ListProbeSourcesResponse, ProbeSourceResponse,
    SyncProbeTrafficResponse, UpdateProbeSourceRequest,
};
use deve_sub_kernel::{ProbeSourceId, SubscriptionId};

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
    State(state): State<ProbeState>,
    admin: AdminUser,
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

    let entry =
        audit::audit_probe_source_create(admin.user.id, &source.id.to_string(), &source.name);
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for probe.source.create");
    }

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
    State(state): State<ProbeState>,
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
    State(state): State<ProbeState>,
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
        (status = 409, description = "Name exists or source changed concurrently", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn update_probe_source(
    State(state): State<ProbeState>,
    admin: AdminUser,
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

    let entry = audit::audit_probe_source_update(admin.user.id, &source.id.to_string());
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for probe.source.update");
    }

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
    State(state): State<ProbeState>,
    admin: AdminUser,
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

    let entry = audit::audit_probe_source_delete(admin.user.id, &source_id.to_string());
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for probe.source.delete");
    }

    Ok(StatusCode::OK)
}

/// `POST /api/v1/probe-sources/{id}/sync` — sync traffic from the external
/// panel (admin only). Triggers an immediate adapter call, writes
/// [`TrafficRecord`] rows for the bound subscription, and persists the new
/// encrypted counter snapshot + sync status (PROBE-001).
#[utoipa::path(
    post,
    path = "/api/v1/probe-sources/{id}/sync",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Probe source ULID")),
    responses(
        (status = 200, description = "Sync completed", body = SyncProbeTrafficResponse),
        (status = 400, description = "Invalid id or source not syncable", body = deve_sub_contract::ErrorResponse),
        (status = 401, description = "Not authenticated", body = deve_sub_contract::ErrorResponse),
        (status = 403, description = "Not an admin", body = deve_sub_contract::ErrorResponse),
        (status = 404, description = "Source not found", body = deve_sub_contract::ErrorResponse),
        (status = 409, description = "Source changed concurrently; retry", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn sync_probe_source(
    State(state): State<ProbeState>,
    admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<SyncProbeTrafficResponse>, (StatusCode, Json<deve_sub_contract::ErrorResponse>)> {
    let source_id = ProbeSourceId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "probe source id is not a valid ULID",
        )
    })?;

    let SyncProbeTrafficResult {
        samples_written,
        snapshot_updated,
    } = probe::sync_probe_traffic(
        state.probe_source_repo.as_ref(),
        state.probe_adapter.as_ref(),
        source_id,
    )
    .await
    .map_err(|e| {
        if let ProbeAppError::Domain(deve_sub_domain::ProbeError::ProbeFailed(msg)) = &e {
            tracing::warn!(error = %msg, "probe sync failed");
        }
        map_probe_error(e)
    })?;

    let source = probe::get_probe_source(state.probe_source_repo.as_ref(), source_id)
        .await
        .map_err(map_probe_error)?;

    let entry = audit::audit_probe_source_sync(admin.user.id, &source_id.to_string());
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for probe.source.sync");
    }

    Ok(Json(SyncProbeTrafficResponse {
        source: source_to_dto(&source),
        samples_written,
        snapshot_updated,
    }))
}

pub(super) fn register(
    router: utoipa_axum::router::OpenApiRouter<crate::AppState>,
) -> utoipa_axum::router::OpenApiRouter<crate::AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(create_probe_source))
        .routes(routes!(list_probe_sources))
        .routes(routes!(get_probe_source))
        .routes(routes!(update_probe_source))
        .routes(routes!(delete_probe_source))
        .routes(routes!(sync_probe_source))
}
