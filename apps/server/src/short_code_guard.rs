//! Failed short-code probes use an isolated, bounded limiter (OUT-013).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use deve_sub_application::LoginRateLimiter;

#[derive(Clone)]
pub(crate) struct GuardState {
    pub limiter: Arc<dyn LoginRateLimiter>,
    pub trust_proxy_headers: bool,
}

pub(crate) async fn guard(
    State(state): State<GuardState>,
    request: Request,
    next: Next,
) -> Response {
    if !request.uri().path().starts_with("/s/") {
        return next.run(request).await;
    }
    let ip = crate::client_ip::resolve(
        request.headers(),
        request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|peer| peer.0),
        state.trust_proxy_headers,
    );
    let Some(ip) = ip else {
        return next.run(request).await;
    };
    // An independent instance uses only the canonical IP dimension. A failed
    // public lookup must never lock an administrator out of the management UI.
    if state.limiter.check(&ip, None).is_err() {
        let mut response = crate::auth::err(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many failed lookups, try again later",
        )
        .into_response();
        response
            .headers_mut()
            .insert("retry-after", HeaderValue::from_static("60"));
        return response;
    }
    let response = next.run(request).await;
    if response.status() == StatusCode::NOT_FOUND {
        state.limiter.record_failure(&ip, None);
    }
    response
}
