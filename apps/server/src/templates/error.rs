//! Error mapper for template management routes.

use axum::http::StatusCode;
use axum::response::Json;
use deve_sub_contract::ErrorResponse;

use crate::auth::err;

/// Map a [`TemplateAppError`] to an HTTP error response with context.
pub(super) fn map_template_app_error(
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
        TemplateAppError::VersionTemplateMismatch => err(
            StatusCode::CONFLICT,
            "version_template_mismatch",
            "version does not belong to the specified template",
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
