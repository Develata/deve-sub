//! Template generation route handler (admin-only).
//!
//! Implements `POST /api/v1/templates/{id}/generate?profile=&mode=`. This
//! route lives in its own file to keep `templates.rs` under the 500-line fuse.
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"Generation
//! pipeline" (M5 Slice 5a, GEN-014).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use deve_sub_application::template;
use deve_sub_contract::{
    ActiveGenerationQuery, ActiveGenerationResponse, ErrorResponse, ExcludedNodeDto, GenerateQuery,
    GenerationResultDto,
};
use deve_sub_kernel::TemplateId;

use crate::AppState;
use crate::auth::{AdminUser, err};

/// `POST /api/v1/templates/{id}/generate?profile=&mode=` — generate a
/// subscription for the given template and target profile (admin). In strict
/// mode, returns 422 with the compatibility report if any node is excluded
/// (GEN-014). In lenient mode, excludes incompatible nodes and continues.
#[utoipa::path(
    post,
    path = "/api/v1/templates/{id}/generate",
    security(("cookie_auth" = [])),
    params(
        ("id" = String, Path, description = "Template ULID"),
        ("profile" = String, Query, description = "Target profile: mihomo, sing-box, xray, v2ray, shadowrocket, uri_list"),
        ("mode" = Option<String>, Query, description = "Generation mode: strict or lenient (default lenient)"),
    ),
    responses(
        (status = 200, description = "Generation result", body = GenerationResultDto),
        (status = 400, description = "Invalid template id or profile", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Template or active version not found", body = ErrorResponse),
        (status = 422, description = "Strict mode: incompatible nodes excluded", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn generate_template(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Query(q): Query<GenerateQuery>,
) -> Result<Json<GenerationResultDto>, (StatusCode, Json<ErrorResponse>)> {
    let template_id = TemplateId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "template id is not a valid ULID",
        )
    })?;

    let mode = match q.mode.as_deref() {
        None | Some("") => deve_sub_domain::GenerationMode::Lenient,
        Some("strict") => deve_sub_domain::GenerationMode::Strict,
        Some("lenient") => deve_sub_domain::GenerationMode::Lenient,
        Some(other) => {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "invalid_mode",
                &format!("mode must be 'strict' or 'lenient', got '{other}'"),
            ));
        }
    };

    let request = deve_sub_domain::GenerationRequest {
        template_id,
        profile: q.profile,
        mode,
    };

    let result = template::generate(
        state.template_repo.as_ref(),
        state.version_repo.as_ref(),
        state.pool_repo.as_ref(),
        state.cache_repo.as_ref(),
        state.pool_meta_repo.as_ref(),
        request,
    )
    .await
    .map_err(map_generation_error)?;

    Ok(Json(generation_result_to_dto(&result)))
}

fn generation_result_to_dto(r: &deve_sub_domain::GenerationResult) -> GenerationResultDto {
    GenerationResultDto {
        content: r.content.clone(),
        profile: r.profile.clone(),
        included_node_ids: r
            .included_node_ids
            .iter()
            .map(|id| id.to_string())
            .collect(),
        excluded: r
            .excluded
            .iter()
            .map(|n| ExcludedNodeDto {
                node_id: n.node_id.to_string(),
                display_name: n.display_name.clone(),
                reason: n.reason.clone(),
            })
            .collect(),
        warnings: r.warnings.clone(),
    }
}

/// Map a [`TemplateAppError`] from the generation pipeline to an HTTP error
/// response.
fn map_generation_error(
    e: deve_sub_application::TemplateAppError,
) -> (StatusCode, Json<ErrorResponse>) {
    use deve_sub_application::TemplateAppError;
    match e {
        TemplateAppError::TemplateNotFound => err(
            StatusCode::NOT_FOUND,
            "template_not_found",
            "template does not exist",
        ),
        TemplateAppError::VersionNotFound => err(
            StatusCode::NOT_FOUND,
            "no_active_version",
            "template has no active version",
        ),
        TemplateAppError::UnknownProfile(p) => err(
            StatusCode::BAD_REQUEST,
            "unknown_profile",
            &format!(
                "profile '{p}' must be one of: mihomo, sing-box, xray, v2ray, shadowrocket, uri_list"
            ),
        ),
        TemplateAppError::Generation(deve_sub_domain::GenerationError::IncompatibleNodes(
            report,
        )) => {
            let excluded_json = serde_json::to_string(
                &report
                    .excluded
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "node_id": n.node_id.to_string(),
                            "display_name": n.display_name,
                            "reason": n.reason,
                        })
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| "[]".to_owned());
            err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "incompatible_nodes",
                &format!(
                    "strict mode: {} node(s) excluded from generation: {excluded_json}",
                    report.excluded.len()
                ),
            )
        }
        TemplateAppError::Emit(msg) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "emit_error",
            &format!("emission failed: {msg}"),
        ),
        TemplateAppError::EmptyOutput => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "empty_output",
            "generated output is empty or invalid",
        ),
        TemplateAppError::NoCompatibleNodes => err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "no_compatible_nodes",
            "no compatible nodes available for generation (all resolved nodes were excluded or unavailable)",
        ),
        TemplateAppError::SpecYamlParse(msg) => {
            err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_spec_yaml", &msg)
        }
        TemplateAppError::Storage(msg) => {
            tracing::warn!(error = %msg, "generation: storage error");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "storage error during generation",
            )
        }
        other => {
            tracing::warn!(error = %other, "generation failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "generation failed",
            )
        }
    }
}

/// `GET /api/v1/templates/{id}/generations/active?profile=` — get the
/// currently active (last successfully published) generation for a template +
/// profile (admin). Returns 404 if no active generation exists. On generation
/// failure, the previous active generation remains served (GEN-015,
/// constraint #19).
#[utoipa::path(
    get,
    path = "/api/v1/templates/{id}/generations/active",
    security(("cookie_auth" = [])),
    params(
        ("id" = String, Path, description = "Template ULID"),
        ("profile" = String, Query, description = "Target profile: mihomo, sing-box, xray, v2ray, shadowrocket, uri_list"),
    ),
    responses(
        (status = 200, description = "Active generation", body = ActiveGenerationResponse),
        (status = 400, description = "Invalid template id or profile", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "No active generation for this template + profile", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn get_active_template_generation(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Query(q): Query<ActiveGenerationQuery>,
) -> Result<Json<ActiveGenerationResponse>, (StatusCode, Json<ErrorResponse>)> {
    let template_id = TemplateId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "template id is not a valid ULID",
        )
    })?;

    if deve_sub_compatibility::ProfileKind::from_kebab(&q.profile).is_none() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "unknown_profile",
            &format!(
                "profile '{}' must be one of: mihomo, sing-box, xray, v2ray, shadowrocket, uri_list",
                q.profile
            ),
        ));
    }

    let entry = template::get_active_generation(state.cache_repo.as_ref(), template_id, &q.profile)
        .await
        .map_err(map_generation_error)?
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "no_active_generation",
                "no active generation exists for this template + profile",
            )
        })?;

    Ok(Json(ActiveGenerationResponse {
        content: entry.content,
        profile: entry.profile,
        template_version: entry.template_version,
        pool_revision: entry.pool_revision,
        cache_key: entry.cache_key,
    }))
}

/// `POST /api/v1/templates/{id}/preview?profile=&mode=` — preview the
/// generation output for a template + profile without publishing (GEN-016).
/// On cache hit, returns the cached (active) content. On cache miss, runs the
/// full pipeline but does NOT store or activate. The returned content is
/// identical to what `generate` would produce (preview consistency, GEN-016).
#[utoipa::path(
    post,
    path = "/api/v1/templates/{id}/preview",
    security(("cookie_auth" = [])),
    params(
        ("id" = String, Path, description = "Template ULID"),
        ("profile" = String, Query, description = "Target profile: mihomo, sing-box, xray, v2ray, shadowrocket, uri_list"),
        ("mode" = Option<String>, Query, description = "Generation mode: strict or lenient (default lenient)"),
    ),
    responses(
        (status = 200, description = "Preview result", body = GenerationResultDto),
        (status = 400, description = "Invalid template id or profile", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Template or active version not found", body = ErrorResponse),
        (status = 422, description = "Strict mode: incompatible nodes excluded", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn preview_template(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Query(q): Query<GenerateQuery>,
) -> Result<Json<GenerationResultDto>, (StatusCode, Json<ErrorResponse>)> {
    let template_id = TemplateId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "template id is not a valid ULID",
        )
    })?;

    let mode = match q.mode.as_deref() {
        None | Some("") => deve_sub_domain::GenerationMode::Lenient,
        Some("strict") => deve_sub_domain::GenerationMode::Strict,
        Some("lenient") => deve_sub_domain::GenerationMode::Lenient,
        Some(other) => {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "invalid_mode",
                &format!("mode must be 'strict' or 'lenient', got '{other}'"),
            ));
        }
    };

    let request = deve_sub_domain::GenerationRequest {
        template_id,
        profile: q.profile,
        mode,
    };

    let result = template::preview(
        state.template_repo.as_ref(),
        state.version_repo.as_ref(),
        state.pool_repo.as_ref(),
        state.cache_repo.as_ref(),
        state.pool_meta_repo.as_ref(),
        request,
    )
    .await
    .map_err(map_generation_error)?;

    Ok(Json(generation_result_to_dto(&result)))
}

/// Register the template generation routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(generate_template))
        .routes(routes!(get_active_template_generation))
        .routes(routes!(preview_template))
}
