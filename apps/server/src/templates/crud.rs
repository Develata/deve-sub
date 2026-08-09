//! Template CRUD, version listing, and rollback route handlers (admin-only).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use deve_sub_application::template::{self, CreateTemplateParams, UpdateTemplateParams};
use deve_sub_contract::{
    CreateTemplateRequest, ErrorResponse, GetTemplateResponse, ListTemplatesQuery,
    ListTemplatesResponse, ListVersionsResponse, RollbackRequest, RollbackTemplateResponse,
    TemplateResponse, UpdateTemplateRequest,
};
use deve_sub_kernel::{TemplateId, TemplateVersionId};

use crate::AppState;
use crate::auth::{AdminUser, err};

use super::error::map_template_app_error;
use super::mappers::{template_to_dto, version_to_dto};

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
pub(super) async fn create_template(
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
pub(super) async fn list_templates(
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

    let template_dtos: Vec<_> = templates.iter().map(template_to_dto).collect();
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
pub(super) async fn get_template(
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
pub(super) async fn update_template(
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
pub(super) async fn delete_template(
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
pub(super) async fn list_versions(
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

    let version_dtos: Vec<_> = versions.iter().map(version_to_dto).collect();
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
        (status = 409, description = "Version belongs to a different template", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
pub(super) async fn rollback_template(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<RollbackRequest>,
) -> Result<Json<RollbackTemplateResponse>, (StatusCode, Json<ErrorResponse>)> {
    let template_id = TemplateId::parse(&id).map_err(|_| {
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

    let version = template::rollback_template(state.version_repo.as_ref(), template_id, version_id)
        .await
        .map_err(|e| map_template_app_error(e, "rollback_template"))?;

    Ok(Json(RollbackTemplateResponse {
        version: version_to_dto(&version),
    }))
}
