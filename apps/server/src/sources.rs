//! Source management route handlers (admin-only).
//!
//! Implements the `/api/v1/sources/*` endpoints: create, list, get, update,
//! and delete. All routes require an authenticated admin via the
//! [`AdminUser`] extractor. See `docs/plan/milestones/M4-sources-and-node-pool.md`.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use deve_sub_application::{
    audit,
    source::{self, CreateSourceParams, UpdateSourceParams},
};
use deve_sub_contract::{
    CreateSourceRequest, ErrorResponse, ListSourcesResponse, SourceDto, SourceFilterRulesDto,
    SourceResponse, SourceTypeDto, UpdateSourceRequest,
};
use deve_sub_domain::{Source, SourceFilterRules, SourceType};
use deve_sub_kernel::SourceId;
use deve_sub_security::mask_url;

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

/// Convert a DTO [`SourceFilterRulesDto`] to the domain type.
pub(crate) fn filter_rules_from_dto(dto: SourceFilterRulesDto) -> SourceFilterRules {
    SourceFilterRules {
        include_protocols: dto.include_protocols,
        exclude_protocols: dto.exclude_protocols,
        include_regions: dto.include_regions,
        exclude_regions: dto.exclude_regions,
    }
}

/// Convert a domain [`SourceFilterRules`] to the DTO type.
pub(crate) fn filter_rules_to_dto(rules: &SourceFilterRules) -> SourceFilterRulesDto {
    SourceFilterRulesDto {
        include_protocols: rules.include_protocols.clone(),
        exclude_protocols: rules.exclude_protocols.clone(),
        include_regions: rules.include_regions.clone(),
        exclude_regions: rules.exclude_regions.clone(),
    }
}

/// Convert a domain [`Source`] to the DTO representation.
fn source_to_dto(source: &Source) -> SourceDto {
    SourceDto {
        id: source.id.to_string(),
        name: source.name.clone(),
        source_type: source_type_to_dto(source.source_type),
        url: mask_url(&source.url),
        auto_update: source.auto_update,
        update_interval_secs: source.update_interval_secs,
        enabled: source.enabled,
        keep_on_fail: source.keep_on_fail,
        filter_rules: source.filter_rules.as_ref().map(filter_rules_to_dto),
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
    admin: AdminUser,
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
            filter_rules: req.filter_rules.map(filter_rules_from_dto),
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

    let entry = audit::audit_source_create(admin.user.id, &source.id.to_string(), &source.name);
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for source.create");
    }

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
    admin: AdminUser,
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
            filter_rules: req.filter_rules.map(filter_rules_from_dto),
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

    let entry = audit::audit_source_update(admin.user.id, &source.id.to_string());
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for source.update");
    }

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
    admin: AdminUser,
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

    let entry = audit::audit_source_delete(admin.user.id, &source_id.to_string());
    if let Err(e) = audit::record_audit_log(state.audit_log_repo.as_ref(), &entry).await {
        tracing::warn!(error = %e, "audit log write failed for source.delete");
    }

    Ok(StatusCode::OK)
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
}
