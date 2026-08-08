//! Template management route handlers (admin-only).
//!
//! Implements the `/api/v1/templates/*` endpoints: create, list, get,
//! update, delete, list versions, and rollback. All routes require an
//! authenticated admin via the [`AdminUser`] extractor. See
//! `docs/plan/milestones/M5-generator-and-v3-template.md`.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use deve_sub_application::template::{self, CreateTemplateParams, UpdateTemplateParams};
use deve_sub_contract::{
    CreateTemplateRequest, ErrorResponse, GetTemplateResponse, ListTemplatesQuery,
    ListTemplatesResponse, ListVersionsResponse, RollbackTemplateResponse, TemplateDto,
    TemplateResponse, TemplateVersionDto, UpdateTemplateRequest,
};
use deve_sub_domain::{SubscriptionTemplate, TemplateVersion};
use deve_sub_kernel::{TemplateId, TemplateVersionId};

use crate::AppState;
use crate::auth::{AdminUser, err, ts_to_iso8601};

/// Convert a domain [`SubscriptionTemplate`] to the DTO representation.
fn template_to_dto(t: &SubscriptionTemplate) -> TemplateDto {
    TemplateDto {
        id: t.id.to_string(),
        name: t.name.clone(),
        description: t.description.clone(),
        active_version: t.active_version,
        active_version_id: t.active_version_id.map(|id| id.to_string()),
        created_at: ts_to_iso8601(t.created_at),
        updated_at: ts_to_iso8601(t.updated_at),
    }
}

/// Convert a domain [`TemplateVersion`] to the DTO representation.
fn version_to_dto(v: &TemplateVersion) -> TemplateVersionDto {
    TemplateVersionDto {
        id: v.id.to_string(),
        template_id: v.template_id.to_string(),
        version: v.version,
        spec_yaml: v.spec_yaml.clone(),
        is_active: v.is_active,
        created_at: ts_to_iso8601(v.created_at),
    }
}

/// `POST /api/v1/templates` — create a new V3 subscription template (admin).
#[utoipa::path(
    post,
    path = "/api/v1/templates",
    security(("cookie_auth" = [])),
    request_body = CreateTemplateRequest,
    responses(
        (status = 201, description = "Template created", body = TemplateResponse),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 409, description = "Name already exists", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn create_template(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(req): Json<CreateTemplateRequest>,
) -> Result<(StatusCode, Json<TemplateResponse>), (StatusCode, Json<ErrorResponse>)> {
    let result = template::create_template(
        state.template_repo.as_ref(),
        state.version_repo.as_ref(),
        CreateTemplateParams {
            name: req.name,
            description: req.description,
            spec_yaml: req.spec_yaml,
        },
    )
    .await
    .map_err(|e| map_template_app_error(e, "create_template"))?;

    Ok((
        StatusCode::CREATED,
        Json(TemplateResponse {
            template: template_to_dto(&result.template),
            version: version_to_dto(&result.version),
        }),
    ))
}

/// `GET /api/v1/templates` — list templates with cursor pagination (admin).
#[utoipa::path(
    get,
    path = "/api/v1/templates",
    security(("cookie_auth" = [])),
    params(
        ("cursor" = Option<String>, Query, description = "Pagination cursor (last template ULID)"),
        ("limit" = Option<u32>, Query, description = "Max templates per page (default 50, max 100)"),
    ),
    responses(
        (status = 200, description = "Template list", body = ListTemplatesResponse),
        (status = 400, description = "Invalid cursor", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn list_templates(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<ListTemplatesQuery>,
) -> Result<Json<ListTemplatesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = q.limit.unwrap_or(50).clamp(1, 100);

    let cursor = q
        .cursor
        .as_deref()
        .map(TemplateId::parse)
        .transpose()
        .map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "cursor is not a valid ULID",
            )
        })?;

    let templates = template::list_templates(state.template_repo.as_ref(), cursor, Some(limit))
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_templates failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to list templates",
            )
        })?;

    let next_cursor = if templates.len() as u32 >= limit {
        templates.last().map(|t| t.id.to_string())
    } else {
        None
    };

    let template_dtos: Vec<TemplateDto> = templates.iter().map(template_to_dto).collect();
    Ok(Json(ListTemplatesResponse {
        templates: template_dtos,
        next_cursor,
    }))
}

/// `GET /api/v1/templates/{id}` — get a template by ID (admin).
#[utoipa::path(
    get,
    path = "/api/v1/templates/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Template ULID")),
    responses(
        (status = 200, description = "Template found", body = GetTemplateResponse),
        (status = 400, description = "Invalid template id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Template not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn get_template(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<GetTemplateResponse>, (StatusCode, Json<ErrorResponse>)> {
    let template_id = TemplateId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "template id is not a valid ULID",
        )
    })?;

    let template = template::get_template(state.template_repo.as_ref(), template_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "get_template failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to get template",
            )
        })?
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "template_not_found",
                "template does not exist",
            )
        })?;

    Ok(Json(GetTemplateResponse {
        template: template_to_dto(&template),
    }))
}

/// `PUT /api/v1/templates/{id}` — update an existing template (admin).
#[utoipa::path(
    put,
    path = "/api/v1/templates/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Template ULID")),
    request_body = UpdateTemplateRequest,
    responses(
        (status = 200, description = "Template updated", body = TemplateResponse),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Template not found", body = ErrorResponse),
        (status = 409, description = "Name already exists", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn update_template(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateTemplateRequest>,
) -> Result<Json<TemplateResponse>, (StatusCode, Json<ErrorResponse>)> {
    let template_id = TemplateId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "template id is not a valid ULID",
        )
    })?;

    let result = template::update_template(
        state.template_repo.as_ref(),
        state.version_repo.as_ref(),
        UpdateTemplateParams {
            id: template_id,
            name: req.name,
            description: req.description,
            spec_yaml: req.spec_yaml,
        },
    )
    .await
    .map_err(|e| map_template_app_error(e, "update_template"))?;

    Ok(Json(TemplateResponse {
        template: template_to_dto(&result.template),
        version: version_to_dto(&result.version),
    }))
}

/// `DELETE /api/v1/templates/{id}` — delete a template (admin).
#[utoipa::path(
    delete,
    path = "/api/v1/templates/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Template ULID")),
    responses(
        (status = 200, description = "Template deleted"),
        (status = 400, description = "Invalid template id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Template not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn delete_template(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let template_id = TemplateId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "template id is not a valid ULID",
        )
    })?;

    template::delete_template(state.template_repo.as_ref(), template_id)
        .await
        .map_err(|e| map_template_app_error(e, "delete_template"))?;

    Ok(StatusCode::OK)
}

/// `GET /api/v1/templates/{id}/versions` — list version history (admin).
#[utoipa::path(
    get,
    path = "/api/v1/templates/{id}/versions",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Template ULID")),
    responses(
        (status = 200, description = "Version history", body = ListVersionsResponse),
        (status = 400, description = "Invalid template id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn list_versions(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<ListVersionsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let template_id = TemplateId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "template id is not a valid ULID",
        )
    })?;

    let versions = template::list_versions(state.version_repo.as_ref(), template_id, Some(100))
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_versions failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to list versions",
            )
        })?;

    let version_dtos: Vec<TemplateVersionDto> = versions.iter().map(version_to_dto).collect();
    Ok(Json(ListVersionsResponse {
        versions: version_dtos,
    }))
}

/// `POST /api/v1/templates/{id}/rollback` — rollback to a specific version
/// (admin).
#[utoipa::path(
    post,
    path = "/api/v1/templates/{id}/rollback",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Template ULID")),
    request_body = RollbackRequest,
    responses(
        (status = 200, description = "Rollback successful", body = RollbackTemplateResponse),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Version not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn rollback_template(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<RollbackRequest>,
) -> Result<Json<RollbackTemplateResponse>, (StatusCode, Json<ErrorResponse>)> {
    let _template_id = TemplateId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "template id is not a valid ULID",
        )
    })?;

    let version_id = TemplateVersionId::parse(&req.version_id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_version_id",
            "version id is not a valid ULID",
        )
    })?;

    let version = template::rollback_template(state.version_repo.as_ref(), version_id)
        .await
        .map_err(|e| map_template_app_error(e, "rollback_template"))?;

    Ok(Json(RollbackTemplateResponse {
        version: version_to_dto(&version),
    }))
}

/// Request body for `POST /api/v1/templates/{id}/rollback`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct RollbackRequest {
    /// The version ULID to activate.
    pub version_id: String,
}

/// Map a [`TemplateAppError`] to an HTTP error response with context.
fn map_template_app_error(
    e: deve_sub_application::TemplateAppError,
    ctx: &str,
) -> (StatusCode, Json<ErrorResponse>) {
    use deve_sub_application::TemplateAppError;
    match e {
        TemplateAppError::InvalidInput(msg) => err(StatusCode::BAD_REQUEST, "invalid_input", &msg),
        TemplateAppError::SpecYamlParse(msg) => {
            err(StatusCode::BAD_REQUEST, "invalid_spec_yaml", &msg)
        }
        TemplateAppError::SpecTooLarge(size, max) => err(
            StatusCode::BAD_REQUEST,
            "spec_too_large",
            &format!("spec is {size} bytes, max {max}"),
        ),
        TemplateAppError::AliasDepthExceeded(depth, max) => err(
            StatusCode::BAD_REQUEST,
            "alias_depth_exceeded",
            &format!("spec nesting depth {depth} exceeds limit {max}"),
        ),
        TemplateAppError::ForbiddenScript(key) => err(
            StatusCode::BAD_REQUEST,
            "forbidden_script",
            &format!("spec contains forbidden script tag: {key}"),
        ),
        TemplateAppError::TemplateNotFound => err(
            StatusCode::NOT_FOUND,
            "template_not_found",
            "template does not exist",
        ),
        TemplateAppError::VersionNotFound => err(
            StatusCode::NOT_FOUND,
            "version_not_found",
            "template version does not exist",
        ),
        TemplateAppError::NameExists => err(
            StatusCode::CONFLICT,
            "name_exists",
            "template name is already taken",
        ),
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

/// Register all template management routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(create_template))
        .routes(routes!(list_templates))
        .routes(routes!(get_template))
        .routes(routes!(update_template))
        .routes(routes!(delete_template))
        .routes(routes!(list_versions))
        .routes(routes!(rollback_template))
}
