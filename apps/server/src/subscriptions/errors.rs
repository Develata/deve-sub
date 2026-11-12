//! Subscription error projection at the HTTP boundary.
use crate::auth::err;
use axum::{http::StatusCode, response::Json};
use deve_sub_contract::ErrorResponse;

/// Map a [`SubscriptionAppError`] to an HTTP error response with context.
pub(super) fn map_subscription_app_error(
    e: deve_sub_application::SubscriptionAppError,
    ctx: &str,
) -> (StatusCode, Json<ErrorResponse>) {
    use deve_sub_application::SubscriptionAppError;
    match e {
        SubscriptionAppError::InvalidGeneration(msg) => {
            err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_generation", &msg)
        }
        SubscriptionAppError::InvalidInput(msg) => {
            err(StatusCode::BAD_REQUEST, "invalid_input", &msg)
        }
        SubscriptionAppError::UnknownProfile(p) => err(
            StatusCode::BAD_REQUEST,
            "unknown_profile",
            &format!("profile '{p}' is not recognized"),
        ),
        SubscriptionAppError::SubscriptionNotFound => err(
            StatusCode::NOT_FOUND,
            "subscription_not_found",
            "subscription does not exist",
        ),
        SubscriptionAppError::TokenNotFound => err(
            StatusCode::NOT_FOUND,
            "token_not_found",
            "subscription token does not exist",
        ),
        SubscriptionAppError::ShortCodeNotFound => err(
            StatusCode::NOT_FOUND,
            "short_code_not_found",
            "short code does not exist",
        ),
        SubscriptionAppError::TempLinkNotFound => err(
            StatusCode::NOT_FOUND,
            "temp_link_not_found",
            "temp link does not exist",
        ),
        SubscriptionAppError::TempLinkInvalid => err(
            StatusCode::NOT_FOUND,
            "temp_link_invalid",
            "temp link is revoked or expired",
        ),
        SubscriptionAppError::TemplateNotFound => err(
            StatusCode::NOT_FOUND,
            "template_not_found",
            "template does not exist",
        ),
        SubscriptionAppError::SlugExists => err(
            StatusCode::CONFLICT,
            "slug_exists",
            "subscription slug is already taken for this owner",
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
