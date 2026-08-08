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
    ChainEdgeDto, CompatibilityReportDto, CreateTemplateRequest, ErrorResponse,
    GetTemplateResponse, GroupResolutionDto, ListTemplatesQuery, ListTemplatesResponse,
    ListVersionsResponse, MissingNodeRefDto, ResolveTemplateResponse, RollbackTemplateResponse,
    TemplateDto, TemplateResponse, TemplateVersionDto, UpdateTemplateRequest,
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

/// `GET /api/v1/templates/{id}/resolve` — resolve the template's nodeSelector
/// and proxyGroups against the live node pool (admin). Read-only: no
/// generation, no caching, no state change.
#[utoipa::path(
    get,
    path = "/api/v1/templates/{id}/resolve",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Template ULID")),
    responses(
        (status = 200, description = "Resolution result", body = ResolveTemplateResponse),
        (status = 400, description = "Invalid template id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Template not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn resolve_template_route(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<ResolveTemplateResponse>, (StatusCode, Json<ErrorResponse>)> {
    let template_id = TemplateId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "template id is not a valid ULID",
        )
    })?;

    let _tmpl = template::get_template(state.template_repo.as_ref(), template_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "resolve: get_template failed");
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

    let version = template::get_active_version(state.version_repo.as_ref(), template_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "resolve: get_active_version failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to get active version",
            )
        })?
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "no_active_version",
                "template has no active version",
            )
        })?;

    let doc = template::parse_template_document(&version.spec_yaml).map_err(|e| {
        tracing::warn!(error = %e, "resolve: spec YAML parse failed");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_spec_yaml",
            "stored spec YAML is invalid",
        )
    })?;

    let resolution = template::resolve_template(&doc, state.pool_repo.as_ref())
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "resolve: resolve_template failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to resolve template",
            )
        })?;

    let chain_graph = deve_sub_domain::ChainGraph::from_groups(&doc.spec.proxy_groups);

    Ok(Json(resolution_to_dto(&resolution, &chain_graph)))
}

/// `GET /api/v1/templates/{id}/compatibility?profile=` — check which
/// resolved nodes are compatible with a target profile (admin). Read-only.
#[utoipa::path(
    get,
    path = "/api/v1/templates/{id}/compatibility",
    security(("cookie_auth" = [])),
    params(
        ("id" = String, Path, description = "Template ULID"),
        ("profile" = String, Query, description = "Target profile: mihomo, sing-box, xray, v2ray, shadowrocket, uri_list"),
    ),
    responses(
        (status = 200, description = "Compatibility report", body = CompatibilityReportDto),
        (status = 400, description = "Invalid template id or profile", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Template not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn check_compatibility_route(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Query(q): Query<CompatibilityQuery>,
) -> Result<Json<CompatibilityReportDto>, (StatusCode, Json<ErrorResponse>)> {
    let template_id = TemplateId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "template id is not a valid ULID",
        )
    })?;

    let profile = deve_sub_compatibility::ProfileKind::from_kebab(&q.profile).ok_or_else(|| {
        err(
            StatusCode::BAD_REQUEST,
            "unknown_profile",
            "profile must be one of: mihomo, sing-box, xray, v2ray, shadowrocket, uri_list",
        )
    })?;

    let _tmpl = template::get_template(state.template_repo.as_ref(), template_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "compatibility: get_template failed");
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

    let version = template::get_active_version(state.version_repo.as_ref(), template_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "compatibility: get_active_version failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to get active version",
            )
        })?
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "no_active_version",
                "template has no active version",
            )
        })?;

    let doc = template::parse_template_document(&version.spec_yaml).map_err(|e| {
        tracing::warn!(error = %e, "compatibility: spec YAML parse failed");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_spec_yaml",
            "stored spec YAML is invalid",
        )
    })?;

    let resolution = template::resolve_template(&doc, state.pool_repo.as_ref())
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "compatibility: resolve_template failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to resolve template",
            )
        })?;

    let mut all_ids = resolution.selected_node_ids;
    for g in &resolution.groups {
        all_ids.extend(g.explicit_node_ids.iter().copied());
        all_ids.extend(g.quick_group_node_ids.iter().copied());
    }
    all_ids.sort();
    all_ids.dedup();

    let report = template::check_compatibility(&all_ids, profile, state.pool_repo.as_ref())
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "compatibility: check_compatibility failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to check compatibility",
            )
        })?;

    Ok(Json(compat_report_to_dto(&report)))
}

fn compat_report_to_dto(r: &deve_sub_domain::CompatibilityReport) -> CompatibilityReportDto {
    CompatibilityReportDto {
        profile: r.profile.clone(),
        included_node_ids: r
            .included_node_ids
            .iter()
            .map(|id| id.to_string())
            .collect(),
        excluded: r
            .excluded
            .iter()
            .map(|n| deve_sub_contract::ExcludedNodeDto {
                node_id: n.node_id.to_string(),
                display_name: n.display_name.clone(),
                reason: n.reason.clone(),
            })
            .collect(),
    }
}

/// Query parameters for `GET /api/v1/templates/{id}/compatibility`.
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
pub struct CompatibilityQuery {
    pub profile: String,
}

fn resolution_to_dto(
    r: &deve_sub_domain::TemplateResolution,
    chain_graph: &deve_sub_domain::ChainGraph,
) -> ResolveTemplateResponse {
    ResolveTemplateResponse {
        selected_node_ids: r
            .selected_node_ids
            .iter()
            .map(|id| id.to_string())
            .collect(),
        selection_missing: r.selection_missing.iter().map(missing_to_dto).collect(),
        groups: r.groups.iter().map(group_resolution_to_dto).collect(),
        chain_edges: chain_graph
            .edges()
            .into_iter()
            .map(|e| ChainEdgeDto {
                from: e.from.to_string(),
                to: e.to.to_string(),
            })
            .collect(),
    }
}

fn missing_to_dto(m: &deve_sub_domain::MissingNodeRef) -> MissingNodeRefDto {
    MissingNodeRefDto {
        node_id: m.node_id.to_string(),
        reason: m.reason.to_string(),
    }
}

fn group_resolution_to_dto(g: &deve_sub_domain::GroupResolution) -> GroupResolutionDto {
    GroupResolutionDto {
        group_name: g.group_name.clone(),
        explicit_node_ids: g
            .explicit_node_ids
            .iter()
            .map(|id| id.to_string())
            .collect(),
        quick_group_node_ids: g
            .quick_group_node_ids
            .iter()
            .map(|id| id.to_string())
            .collect(),
        missing: g.missing.iter().map(missing_to_dto).collect(),
    }
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
        .routes(routes!(resolve_template_route))
        .routes(routes!(check_compatibility_route))
}
