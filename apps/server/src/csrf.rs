//! CSRF protection via `Origin` header validation (SEC-010).
//!
//! For state-changing HTTP methods (POST, PUT, DELETE, PATCH), the middleware
//! checks that the `Origin` header — when present — matches the request's
//! `Host` header. If `Origin` is absent, the request is allowed because
//! `SameSite=Lax` cookies (set on all session cookies) provide the primary
//! CSRF defense: cross-site requests cannot carry the session cookie.
//!
//! This dual-layer approach (SameSite=Lax + Origin validation) follows the
//! OWASP CSRF prevention cheat sheet. Non-browser clients (CLI, curl) that
//! do not send an `Origin` header are unaffected.

use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use deve_sub_contract::ErrorResponse;

/// Axum middleware function that validates the `Origin` header on
/// state-changing requests.
///
/// WHY: when `Origin` is absent, the request is allowed because:
/// (1) `SameSite=Lax` cookies (set on all session cookies) block cross-site
/// POST requests from carrying the session cookie — this is the primary
/// CSRF defense.
/// (2) Non-browser clients (CLI, curl) do not send `Origin` and should not
/// be blocked.
/// Login CSRF (attacker establishes a session in the victim's browser) is a
/// theoretical risk when `Origin` is absent, but modern browsers always
/// send `Origin` on cross-site POSTs. A stricter mode requiring `Origin` on
/// login is a future improvement.
pub async fn csrf_guard(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    if is_state_changing(&method) {
        let headers = request.headers();
        let origin = headers.get("origin").and_then(|v| v.to_str().ok());
        let host = headers.get("host").and_then(|v| v.to_str().ok());

        match (origin, host) {
            (Some(origin), Some(host)) => {
                if !origin_matches_host(origin, host) {
                    return csrf_error_response();
                }
            }
            // WHY: a browser request always includes Host (HTTP/1.1
            // requires it). If Origin is present but Host is absent, the
            // request is malformed or adversarial — reject it.
            (Some(_), None) => return csrf_error_response(),
            // No Origin → allowed (SameSite=Lax provides the primary
            // defense; non-browser clients don't send Origin).
            (None, _) => {}
        }
    }
    next.run(request).await
}

/// Whether the HTTP method is state-changing and thus subject to CSRF
/// validation.
fn is_state_changing(method: &Method) -> bool {
    matches!(
        method,
        &Method::POST | &Method::PUT | &Method::DELETE | &Method::PATCH
    )
}

/// Extract the host:port portion from an `Origin` header value and compare
/// it to the `Host` header.
///
/// `Origin` is `scheme://host:port` (or `scheme://host`). We strip the
/// scheme prefix and any trailing path, then compare the remaining
/// `host:port` to the `Host` header value using case-insensitive matching
/// (HTTP host headers are case-insensitive per RFC 7230 §5.4). A mismatch
/// indicates a cross-origin request, which is rejected.
fn origin_matches_host(origin: &str, host: &str) -> bool {
    let origin_host = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .unwrap_or(origin);
    // Strip any path component (Origin should not have one, but be safe).
    let origin_host = origin_host.split('/').next().unwrap_or(origin_host);
    origin_host.eq_ignore_ascii_case(host)
}

/// Build a 403 JSON error response for a CSRF validation failure.
fn csrf_error_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorResponse {
            error: "csrf_error".to_owned(),
            message: "origin header does not match host".to_owned(),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_matches_host_same_port() {
        assert!(origin_matches_host(
            "http://localhost:8080",
            "localhost:8080"
        ));
    }

    #[test]
    fn origin_matches_host_https() {
        assert!(origin_matches_host(
            "https://example.com:443",
            "example.com:443"
        ));
    }

    #[test]
    fn origin_mismatch_different_host() {
        assert!(!origin_matches_host("https://evil.com", "example.com:8080"));
    }

    #[test]
    fn origin_mismatch_different_port() {
        assert!(!origin_matches_host(
            "http://localhost:9090",
            "localhost:8080"
        ));
    }

    #[test]
    fn is_state_changing_post() {
        assert!(is_state_changing(&Method::POST));
    }

    #[test]
    fn is_state_changing_get() {
        assert!(!is_state_changing(&Method::GET));
    }
}
