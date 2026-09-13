//! HTTP request tracing with secret-path redaction (DS-AUD-029).
//!
//! The global `TraceLayer` logs the request URI for every request. Public
//! subscription delivery routes (`/sub/{token}`, `/sub/{token}/{profile}`,
//! `/s/{code}`, `/s/{code}/{profile}`) carry the raw delivery token or short
//! code in the path — a secret that must not appear in logs. This module
//! provides a custom span builder that replaces those path segments with
//! `***` before they enter the tracing span.
//!
//! See ADR-0007 §"Redaction boundary" and the constitution §"Data and
//! security": sensitive fields are redacted in logs.

use std::fmt::Debug;

use axum::http::Request;
use tower_http::trace::{MakeSpan, TraceLayer};
use tracing::Span;

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

/// A `MakeSpan` implementation that records the redacted URI and HTTP method.
#[derive(Debug, Clone)]
pub struct RedactingMakeSpan;

impl<B> MakeSpan<B> for RedactingMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        let method = request.method().as_str();
        let uri = redacted_uri(request);
        tracing::debug_span!(
            "http.request",
            method = %method,
            uri = %uri,
        )
    }
}

/// Build a `TraceLayer` that redacts secret path segments before logging.
///
/// The returned layer is a `TraceLayer` configured for HTTP tracing with a
/// custom span builder ([`RedactingMakeSpan`]) that replaces `/sub/{token}`
/// and `/s/{code}` path segments with `***` (DS-AUD-029).
pub fn redacting_trace_layer() -> TraceLayer<
    tower_http::classify::SharedClassifier<tower_http::classify::ServerErrorsAsFailures>,
    RedactingMakeSpan,
> {
    TraceLayer::new_for_http().make_span_with(RedactingMakeSpan)
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
