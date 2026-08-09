//! Public subscription delivery routes: `/sub/{token}/{profile}`, `/sub/{token}`,
//! and `/s/{code}/{profile}`, `/s/{code}`.
//!
//! These routes are the public delivery surface (M6 Slice 2 + Slice 3). They
//! use path-token or short-code authentication (no cookie, no `AdminUser`
//! guard) and are intentionally excluded from the OpenAPI spec. See
//! `docs/contracts/module-boundaries.md` §"Delivery" and
//! `docs/plan/milestones/M6-subscription-distribution.md` §"Delivery
//! pipeline".
//!
//! Security: bad token, disabled subscription, deleted subscription, expired
//! temp link, and inactive user all return 404 with a generic body — the
//! response must not reveal whether the token, subscription, or owner exists
//! (OUT-009).

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use deve_sub_application::subscription;

use crate::AppState;

/// HTTP `Cache-Control` header value for delivery responses.
const CACHE_CONTROL: &str = "private, no-cache";

/// `GET /sub/{token}/{profile}` — deliver a subscription for an explicit
/// profile.
async fn deliver_with_profile(
    State(state): State<AppState>,
    Path((token, profile)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());
    deliver_token(&state, &token, Some(&profile), ua.as_deref(), &headers).await
}

/// `GET /sub/{token}` — deliver a subscription with User-Agent auto-detect.
async fn deliver_auto(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Response {
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());
    deliver_token(&state, &token, None, ua.as_deref(), &headers).await
}

/// `GET /s/{code}/{profile}` — deliver via short code for an explicit profile.
async fn deliver_short_code_with_profile(
    State(state): State<AppState>,
    Path((code, profile)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    deliver_code(&state, &code, Some(&profile), None, &headers).await
}

/// `GET /s/{code}` — deliver via short code with User-Agent auto-detect.
async fn deliver_short_code_auto(
    State(state): State<AppState>,
    Path(code): Path<String>,
    headers: HeaderMap,
) -> Response {
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());
    deliver_code(&state, &code, None, ua.as_deref(), &headers).await
}

/// Token-based delivery: try permanent token first, fall back to temp link
/// on `TokenNotFound`.
///
/// WHY fallback: permanent tokens and temp links share the same URL path
/// `/sub/{token}` and the same HMAC purpose. The permanent token table is
/// queried first; only if that misses do we query the temp link table. This
/// avoids a second HMAC + DB round-trip for the common permanent-token case.
async fn deliver_token(
    state: &AppState,
    token: &str,
    profile: Option<&str>,
    user_agent: Option<&str>,
    headers: &HeaderMap,
) -> Response {
    let deps = make_deps(state);
    let result = subscription::deliver_subscription(&deps, token, profile, user_agent).await;

    let result = match result {
        Ok(d) => return ok_or_304(d, headers),
        Err(subscription::SubscriptionAppError::TokenNotFound) => {
            subscription::deliver_by_temp_link(&deps, token, profile, user_agent).await
        }
        Err(e) => return map_delivery_error(e),
    };

    match result {
        Ok(d) => ok_or_304(d, headers),
        Err(e) => map_delivery_error(e),
    }
}

/// Short-code delivery: resolve code → subscription → standard pipeline.
async fn deliver_code(
    state: &AppState,
    code: &str,
    profile: Option<&str>,
    user_agent: Option<&str>,
    headers: &HeaderMap,
) -> Response {
    let deps = make_deps(state);
    let result = subscription::deliver_by_short_code(&deps, code, profile, user_agent).await;

    match result {
        Ok(d) => ok_or_304(d, headers),
        Err(e) => map_delivery_error(e),
    }
}

fn make_deps(state: &AppState) -> subscription::DeliveryDeps<'_> {
    subscription::DeliveryDeps {
        token_repo: state.subscription_token_repo.as_ref(),
        short_code_repo: state.short_code_repo.as_ref(),
        temp_link_repo: state.temp_link_repo.as_ref(),
        sub_repo: state.subscription_repo.as_ref(),
        user_repo: state.user_repo.as_ref(),
        template_repo: state.template_repo.as_ref(),
        version_repo: state.version_repo.as_ref(),
        pool_repo: state.pool_repo.as_ref(),
        cache_repo: state.cache_repo.as_ref(),
        pool_meta_repo: state.pool_meta_repo.as_ref(),
        master_key: state.master_key.as_ref(),
    }
}

fn ok_or_304(delivery: subscription::DeliveryResult, headers: &HeaderMap) -> Response {
    let if_none_match = headers
        .get("if-none-match")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    if let Some(inm) = if_none_match
        && etag_matches(&inm, &delivery.etag)
    {
        return not_modified(&delivery.etag);
    }

    ok_response(delivery)
}

/// Check whether the `If-None-Match` header matches the current ETag.
///
/// Handles both exact match and `*` (always matches).
fn etag_matches(if_none_match: &str, etag: &str) -> bool {
    if if_none_match == "*" {
        return true;
    }
    if_none_match
        .split(',')
        .map(|s| s.trim())
        .any(|s| s == etag || s == format!("W/{}", etag))
}

/// Build a 200 OK response with delivery headers and content.
///
/// WHY `Response::builder` instead of `(StatusCode, AppendHeaders, String)`:
/// the `String` impl of `IntoResponse` inserts `content-type: text/plain`,
/// which would override the `AppendHeaders` value. Building the response from
/// raw parts lets us set `content-type` exactly once.
fn ok_response(delivery: subscription::DeliveryResult) -> Response {
    let mut builder = Response::builder().status(StatusCode::OK);
    builder = builder.header("cache-control", CACHE_CONTROL);
    builder = builder.header("etag", &delivery.etag);
    builder = builder.header("content-type", delivery.content_type);
    builder = builder.header("content-disposition", &delivery.content_disposition);
    builder = builder.header("subscription-userinfo", &delivery.subscription_userinfo);
    builder
        .body(Body::from(delivery.content))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Build a 304 Not Modified response with ETag and Cache-Control.
fn not_modified(etag: &str) -> Response {
    Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .header("cache-control", CACHE_CONTROL)
        .header("etag", etag)
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Build a 404 Not Found response (generic, no existence leak).
fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

/// Build a 503 Service Unavailable response (generation failure).
fn service_unavailable() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, "Service Unavailable").into_response()
}

/// Map a [`subscription::SubscriptionAppError`] to a delivery HTTP response.
///
/// All token-resolution and access-control failures return 404 (OUT-009).
/// Generation failure returns 503 (constraint #19). Infrastructure errors
/// return 500.
fn map_delivery_error(e: subscription::SubscriptionAppError) -> Response {
    use subscription::SubscriptionAppError;
    match e {
        SubscriptionAppError::TokenNotFound
        | SubscriptionAppError::ShortCodeNotFound
        | SubscriptionAppError::TempLinkInvalid
        | SubscriptionAppError::TempLinkNotFound
        | SubscriptionAppError::SubscriptionNotFound
        | SubscriptionAppError::SubscriptionDisabled
        | SubscriptionAppError::UserInactive
        | SubscriptionAppError::UnknownProfile(_) => not_found(),
        SubscriptionAppError::GenerationFailed(msg) => {
            tracing::warn!(error = %msg, "delivery: generation failed");
            service_unavailable()
        }
        SubscriptionAppError::Storage(msg) => {
            tracing::warn!(error = %msg, "delivery: storage error");
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
        }
        other => {
            tracing::warn!(error = %other, "delivery: unexpected error");
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
        }
    }
}

/// Register the public delivery routes on the given router.
///
/// These routes are NOT registered via `OpenApiRouter` (they use path tokens
/// or short codes, not cookie auth) and are intentionally excluded from the
/// OpenAPI spec.
pub fn register_delivery_routes(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/sub/{token}/{profile}", get(deliver_with_profile))
        .route("/sub/{token}", get(deliver_auto))
        .route("/s/{code}/{profile}", get(deliver_short_code_with_profile))
        .route("/s/{code}", get(deliver_short_code_auto))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_matches_exact() {
        assert!(etag_matches("\"abc\"", "\"abc\""));
    }

    #[test]
    fn etag_matches_star() {
        assert!(etag_matches("*", "\"abc\""));
    }

    #[test]
    fn etag_no_match() {
        assert!(!etag_matches("\"def\"", "\"abc\""));
    }

    #[test]
    fn etag_matches_multiple() {
        assert!(etag_matches("\"def\", \"abc\"", "\"abc\""));
    }

    #[test]
    fn etag_matches_weak_prefix() {
        assert!(etag_matches("W/\"abc\"", "\"abc\""));
    }
}
