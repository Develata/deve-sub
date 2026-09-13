//! Read the active snapshot without coupling the editor to paginated history.

use super::{error::map_template_app_error, mappers::version_to_dto};
use crate::{
    auth::{AdminUser, err},
    state::TemplateState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use deve_sub_application::template;
use deve_sub_contract::{ActiveTemplateVersionResponse, ErrorResponse};
use deve_sub_kernel::TemplateId;

/// `GET /templates/{id}/versions/active` — current editable version.
#[utoipa::path(
    get, path = "/api/v1/templates/{id}/versions/active",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Template ULID")),
    responses(
        (status = 200, description = "Active version", body = ActiveTemplateVersionResponse),
        (status = 400, description = "Invalid ID", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an administrator", body = ErrorResponse),
        (status = 404, description = "No active version", body = ErrorResponse),
        (status = 500, description = "Storage failure", body = ErrorResponse),
    )
)]
pub(super) async fn get_active_version(
    State(state): State<TemplateState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<ActiveTemplateVersionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let id = TemplateId::parse(&id)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_id", "invalid template ID"))?;
    let version = template::get_active_version(state.version_repo.as_ref(), id)
        .await
        .map_err(|e| map_template_app_error(e, "get_active_version"))?
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "version_not_found",
                "template has no active version",
            )
        })?;
    Ok(Json(ActiveTemplateVersionResponse {
        version: version_to_dto(&version),
    }))
}
