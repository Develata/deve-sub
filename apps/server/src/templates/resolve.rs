//! Template resolution and compatibility route handlers (admin-only, read-only).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use deve_sub_application::template;
use deve_sub_contract::{
    CompatibilityQuery, CompatibilityReportDto, ErrorResponse, ResolveTemplateResponse,
};
use deve_sub_kernel::TemplateId;

use crate::AppState;
use crate::auth::{AdminUser, err};

use super::mappers::{compat_report_to_dto, resolution_to_dto};

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
pub(super) async fn resolve_template_route(
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
pub(super) async fn check_compatibility_route(
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
