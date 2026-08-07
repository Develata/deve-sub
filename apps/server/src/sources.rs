//! Source management route handlers (admin-only).
//!
//! Implements the `/api/v1/sources/*` endpoints: create, list, get, update,
//! and delete. All routes require an authenticated admin via the
//! [`AdminUser`] extractor. See `docs/plan/milestones/M4-sources-and-node-pool.md`.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use deve_sub_application::source::{self, CreateSourceParams, UpdateSourceParams};
use deve_sub_contract::{
    CreateSourceRequest, ErrorResponse, ListSourcesResponse, ReconcileCountsDto,
    RefreshSourceResponse, SourceDto, SourceResponse, SourceTypeDto, UpdateSourceRequest,
};
use deve_sub_domain::{Source, SourceType};
use deve_sub_kernel::SourceId;

use crate::AppState;
use crate::auth::{AdminUser, err, ts_to_iso8601};

/// Query parameters for `GET /api/v1/sources` (cursor pagination).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ListSourcesQuery {
    /// Maximum number of sources to return (1-100, default 20).
    #[serde(default = "default_page_size")]
    pub limit: u32,
    /// Pagination cursor — the ULID of the last source from the previous page.
    pub cursor: Option<String>,
}

fn default_page_size() -> u32 {
    20
}

/// Convert a domain [`SourceType`] to the DTO variant.
pub(crate) fn source_type_to_dto(t: SourceType) -> SourceTypeDto {
    match t {
        SourceType::Auto => SourceTypeDto::Auto,
        SourceType::Base64 => SourceTypeDto::Base64,
        SourceType::UriList => SourceTypeDto::UriList,
        SourceType::MihomoYaml => SourceTypeDto::MihomoYaml,
        SourceType::SingboxJson => SourceTypeDto::SingboxJson,
        SourceType::XrayJson => SourceTypeDto::XrayJson,
        SourceType::V2rayJson => SourceTypeDto::V2rayJson,
        SourceType::Shadowrocket => SourceTypeDto::Shadowrocket,
    }
}

/// Convert a DTO [`SourceTypeDto`] to the domain variant.
pub(crate) fn source_type_from_dto(t: SourceTypeDto) -> SourceType {
    match t {
        SourceTypeDto::Auto => SourceType::Auto,
        SourceTypeDto::Base64 => SourceType::Base64,
        SourceTypeDto::UriList => SourceType::UriList,
        SourceTypeDto::MihomoYaml => SourceType::MihomoYaml,
        SourceTypeDto::SingboxJson => SourceType::SingboxJson,
        SourceTypeDto::XrayJson => SourceType::XrayJson,
        SourceTypeDto::V2rayJson => SourceType::V2rayJson,
        SourceTypeDto::Shadowrocket => SourceType::Shadowrocket,
    }
}

/// Convert a domain [`Source`] to the DTO representation.
fn source_to_dto(source: &Source) -> SourceDto {
    SourceDto {
        id: source.id.to_string(),
        name: source.name.clone(),
        source_type: source_type_to_dto(source.source_type),
        url: source.url.clone(),
        auto_update: source.auto_update,
        update_interval_secs: source.update_interval_secs,
        enabled: source.enabled,
        keep_on_fail: source.keep_on_fail,
        created_at: ts_to_iso8601(source.created_at),
    }
}

/// `POST /api/v1/sources` — create a new subscription source (admin only).
#[utoipa::path(
    post,
    path = "/api/v1/sources",
    security(("cookie_auth" = [])),
    request_body = CreateSourceRequest,
    responses(
        (status = 201, description = "Source created", body = SourceResponse),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 409, description = "Name already exists", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn create_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(req): Json<CreateSourceRequest>,
) -> Result<(StatusCode, Json<SourceResponse>), (StatusCode, Json<ErrorResponse>)> {
    let source = source::create_source(
        state.source_repo.as_ref(),
        CreateSourceParams {
            name: req.name,
            source_type: source_type_from_dto(req.source_type),
            url: req.url,
            auto_update: req.auto_update,
            update_interval_secs: req.update_interval_secs,
            keep_on_fail: req.keep_on_fail,
        },
    )
    .await
    .map_err(|e| match e {
        source::SourceAppError::InvalidInput(msg) => {
            err(StatusCode::BAD_REQUEST, "invalid_input", msg)
        }
        source::SourceAppError::NameExists => err(
            StatusCode::CONFLICT,
            "name_exists",
            "source name is already taken",
        ),
        other => {
            tracing::warn!(error = %other, "create_source failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to create source",
            )
        }
    })?;

    Ok((
        StatusCode::CREATED,
        Json(SourceResponse {
            source: source_to_dto(&source),
        }),
    ))
}

/// `GET /api/v1/sources` — list sources with cursor pagination (admin only).
#[utoipa::path(
    get,
    path = "/api/v1/sources",
    security(("cookie_auth" = [])),
    params(
        ("limit" = Option<u32>, Query, description = "Max sources per page (1-100, default 20)"),
        ("cursor" = Option<String>, Query, description = "Pagination cursor (last source ULID)"),
    ),
    responses(
        (status = 200, description = "Source list", body = ListSourcesResponse),
        (status = 400, description = "Invalid cursor", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn list_sources(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<ListSourcesQuery>,
) -> Result<Json<ListSourcesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = q.limit.clamp(1, 100);

    let cursor = q
        .cursor
        .as_deref()
        .map(SourceId::parse)
        .transpose()
        .map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "cursor is not a valid ULID",
            )
        })?;

    let sources = source::list_sources(state.source_repo.as_ref(), cursor, limit)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_sources failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to list sources",
            )
        })?;

    let next_cursor = if sources.len() as u32 >= limit {
        sources.last().map(|s| s.id.to_string())
    } else {
        None
    };

    let source_dtos: Vec<SourceDto> = sources.iter().map(source_to_dto).collect();
    Ok(Json(ListSourcesResponse {
        sources: source_dtos,
        next_cursor,
    }))
}

/// `GET /api/v1/sources/{id}` — get a source by ID (admin only).
#[utoipa::path(
    get,
    path = "/api/v1/sources/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Source ULID")),
    responses(
        (status = 200, description = "Source found", body = SourceResponse),
        (status = 400, description = "Invalid source id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Source not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn get_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<SourceResponse>, (StatusCode, Json<ErrorResponse>)> {
    let source_id = SourceId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "source id is not a valid ULID",
        )
    })?;

    let source = source::get_source(state.source_repo.as_ref(), source_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "get_source failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to get source",
            )
        })?
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "source_not_found",
                "source does not exist",
            )
        })?;

    Ok(Json(SourceResponse {
        source: source_to_dto(&source),
    }))
}

/// `PUT /api/v1/sources/{id}` — update an existing source (admin only).
#[utoipa::path(
    put,
    path = "/api/v1/sources/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Source ULID")),
    request_body = UpdateSourceRequest,
    responses(
        (status = 200, description = "Source updated", body = SourceResponse),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Source not found", body = ErrorResponse),
        (status = 409, description = "Name already exists", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn update_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateSourceRequest>,
) -> Result<Json<SourceResponse>, (StatusCode, Json<ErrorResponse>)> {
    let source_id = SourceId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "source id is not a valid ULID",
        )
    })?;

    let source = source::update_source(
        state.source_repo.as_ref(),
        UpdateSourceParams {
            id: source_id,
            name: req.name,
            source_type: source_type_from_dto(req.source_type),
            url: req.url,
            auto_update: req.auto_update,
            update_interval_secs: req.update_interval_secs,
            enabled: req.enabled,
            keep_on_fail: req.keep_on_fail,
        },
    )
    .await
    .map_err(|e| match e {
        source::SourceAppError::InvalidInput(msg) => {
            err(StatusCode::BAD_REQUEST, "invalid_input", msg)
        }
        source::SourceAppError::SourceNotFound => err(
            StatusCode::NOT_FOUND,
            "source_not_found",
            "source does not exist",
        ),
        source::SourceAppError::NameExists => err(
            StatusCode::CONFLICT,
            "name_exists",
            "source name is already taken",
        ),
        other => {
            tracing::warn!(error = %other, "update_source failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to update source",
            )
        }
    })?;

    Ok(Json(SourceResponse {
        source: source_to_dto(&source),
    }))
}

/// `DELETE /api/v1/sources/{id}` — delete a source (admin only).
#[utoipa::path(
    delete,
    path = "/api/v1/sources/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Source ULID")),
    responses(
        (status = 200, description = "Source deleted"),
        (status = 400, description = "Invalid source id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Source not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn delete_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let source_id = SourceId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "source id is not a valid ULID",
        )
    })?;

    source::delete_source(state.source_repo.as_ref(), source_id)
        .await
        .map_err(|e| match e {
            source::SourceAppError::SourceNotFound => err(
                StatusCode::NOT_FOUND,
                "source_not_found",
                "source does not exist",
            ),
            other => {
                tracing::warn!(error = %other, "delete_source failed");
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "failed to delete source",
                )
            }
        })?;

    Ok(StatusCode::OK)
}

/// `POST /api/v1/sources/{id}/refresh` — refresh a source (admin only).
///
/// Fetches the subscription URL, parses the content, and reconciles the
/// parsed nodes into the pool. On fetch or parse failure, the last
/// successful snapshot remains active (constraint #19).
#[utoipa::path(
    post,
    path = "/api/v1/sources/{id}/refresh",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Source ULID")),
    responses(
        (status = 200, description = "Source refreshed", body = RefreshSourceResponse),
        (status = 400, description = "Invalid source id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Source not found", body = ErrorResponse),
        (status = 502, description = "Fetch or parse failed", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn refresh_source(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<RefreshSourceResponse>, (StatusCode, Json<ErrorResponse>)> {
    let source_id = SourceId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "source id is not a valid ULID",
        )
    })?;

    let result = source::refresh_source(
        state.source_repo.as_ref(),
        state.snapshot_repo.as_ref(),
        state.pool_repo.as_ref(),
        state.fetcher.as_ref(),
        state.geoip.as_ref(),
        source_id,
    )
    .await
    .map_err(|e| match e {
        source::SourceAppError::SourceNotFound => err(
            StatusCode::NOT_FOUND,
            "source_not_found",
            "source does not exist",
        ),
        source::SourceAppError::Fetch(fe) => {
            tracing::warn!(error = %fe, "source fetch failed");
            err(
                StatusCode::BAD_GATEWAY,
                "fetch_failed",
                "failed to fetch subscription content",
            )
        }
        source::SourceAppError::Parse(pe) => {
            tracing::warn!(error = %pe, "source parse failed");
            err(
                StatusCode::BAD_GATEWAY,
                "parse_failed",
                "failed to parse subscription content",
            )
        }
        other => {
            tracing::warn!(error = %other, "refresh_source failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to refresh source",
            )
        }
    })?;

    Ok(Json(RefreshSourceResponse {
        snapshot_id: result.snapshot.id.to_string(),
        version: result.snapshot.version,
        not_modified: result.not_modified,
        node_count: result.snapshot.node_count,
        reconcile: ReconcileCountsDto {
            new_nodes: result.reconcile.new_nodes,
            duplicate_nodes: result.reconcile.duplicate_nodes,
            reactivated_nodes: result.reconcile.reactivated_nodes,
            missing_nodes: result.reconcile.missing_nodes,
        },
    }))
}

/// Register all source management routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(create_source))
        .routes(routes!(list_sources))
        .routes(routes!(get_source))
        .routes(routes!(update_source))
        .routes(routes!(delete_source))
        .routes(routes!(refresh_source))
}
