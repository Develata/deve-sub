//! HTTP completion tracing with server-owned correlation IDs and path redaction.
//!
//! Public delivery paths carry secret tokens or short codes. Only recognized
//! client profile suffixes survive redaction; queries, headers and bodies are
//! excluded. See ADR-0007 and M10's log lifecycle (LOG-001).

use std::time::Instant;

use axum::{
    body::Body,
    http::{HeaderValue, Request},
    middleware::Next,
    response::Response,
};
use deve_sub_kernel::AuditLogId;
use tracing::Instrument;

const REDACTED: &str = "***";

/// Build a tracing span for an HTTP request with secret paths redacted.
///
/// Replacement rules:
/// - `/sub/{token}` → `/sub/***`
/// - `/sub/{token}/{profile}` → `/sub/***/{profile}`
/// - `/s/{code}` → `/s/***`
/// - `/s/{code}/{profile}` → `/s/***/{profile}`
///
/// Recognized profile names are preserved. Unknown suffixes, including
/// extra path segments on 404 requests, are hidden with the credential.
pub fn redacted_uri<B>(request: &Request<B>) -> String {
    let uri = request.uri();
    let path = uri.path();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    let redacted_segments: Vec<String> = match segments.as_slice() {
        [first, _, rest @ ..] if matches!(*first, "sub" | "s") => {
            let mut result = vec![first.to_string(), REDACTED.to_string()];
            if let [profile] = rest {
                // A malformed profile may itself contain a copied credential.
                if matches!(
                    *profile,
                    "mihomo"
                        | "clash"
                        | "sing-box"
                        | "xray"
                        | "v2ray"
                        | "shadowrocket"
                        | "uri_list"
                        | "json"
                ) {
                    result.push((*profile).to_string());
                } else {
                    result.push(REDACTED.to_string());
                }
            } else if !rest.is_empty() {
                result.push(REDACTED.to_string());
            }
            result
        }
        _ => return path.to_owned(),
    };

    let mut result = String::with_capacity(path.len());
    for seg in &redacted_segments {
        result.push('/');
        result.push_str(seg);
    }
    if path.ends_with('/') {
        result.push('/');
    }
    result
}

/// Record a completion event and correlate handler events without accepting
/// attacker-controlled request IDs, headers, query strings or request bodies.
pub async fn trace_request(mut request: Request<Body>, next: Next) -> Response {
    let request_id = AuditLogId::new().to_string();
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        request.headers_mut().insert("x-request-id", value);
    }
    let uri = redacted_uri(&request);
    let method = request.method().clone();
    let quiet = (uri.starts_with("/health/")
        || (!uri.starts_with("/api/") && !uri.starts_with("/sub/") && !uri.starts_with("/s/")))
        && method == axum::http::Method::GET;
    let span = tracing::info_span!("http.request", %request_id, method = %method, uri = %uri);
    async move {
        let start = Instant::now();
        let mut response = next.run(request).await;
        let status = response.status().as_u16();
        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
        if status >= 500 {
            tracing::error!(status, duration_ms, "http request completed");
        } else if status >= 400 {
            tracing::warn!(status, duration_ms, "http request completed");
        } else if quiet {
            tracing::debug!(status, duration_ms, "http request completed");
        } else {
            tracing::info!(status, duration_ms, "http request completed");
        }
        // ULIDs contain only ASCII letters and digits, always valid in a header.
        if let Ok(value) = HeaderValue::from_str(&request_id) {
            response.headers_mut().insert("x-request-id", value);
        }
        response
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, Uri};

    fn req_uri(path: &str) -> Request<()> {
        let uri = Uri::builder()
            .path_and_query(path)
            .build()
            .unwrap_or_default();
        Request::builder().uri(uri).body(()).unwrap()
    }

    #[test]
    fn redacts_sub_token() {
        let req = req_uri("/sub/abc123secret");
        assert_eq!(redacted_uri(&req), "/sub/***");
    }

    #[test]
    fn redacts_sub_token_preserves_profile() {
        let req = req_uri("/sub/abc123secret/clash");
        assert_eq!(redacted_uri(&req), "/sub/***/clash");
    }

    #[test]
    fn redacts_s_short_code() {
        let req = req_uri("/s/xyz789");
        assert_eq!(redacted_uri(&req), "/s/***");
    }

    #[test]
    fn redacts_s_short_code_preserves_profile() {
        let req = req_uri("/s/xyz789/sing-box");
        assert_eq!(redacted_uri(&req), "/s/***/sing-box");
    }

    #[test]
    fn preserves_api_paths() {
        let req = req_uri("/api/v1/sources/01HTEST000/refresh");
        assert_eq!(redacted_uri(&req), "/api/v1/sources/01HTEST000/refresh");
    }

    #[test]
    fn preserves_root() {
        let req = req_uri("/");
        assert_eq!(redacted_uri(&req), "/");
    }

    #[test]
    fn malformed_delivery_paths_do_not_leak_credentials() {
        for path in [
            "/sub/fixture-secret/mihomo/extra",
            "/s/fixture-secret/sing-box/extra",
            "//sub//fixture-secret///mihomo/extra",
            "/sub/fixture-secret/fixture-secret",
        ] {
            assert!(!redacted_uri(&req_uri(path)).contains("fixture-secret"));
        }
    }

    #[test]
    fn preserves_trailing_slash() {
        let req = req_uri("/sub/secret/");
        assert_eq!(redacted_uri(&req), "/sub/***/");
    }
}
