//! Trusted, canonical client address for HTTP admission controls (SEC-007).

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::{HeaderMap, request::Parts};

use crate::state::AuthState;

/// Client IP resolved using the explicit proxy trust policy and transport peer.
pub(crate) struct ClientIp(pub Option<String>);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
    AuthState: axum::extract::FromRef<S>,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = <AuthState as axum::extract::FromRef<S>>::from_ref(state);
        Ok(Self(resolve(
            &parts.headers,
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|peer| peer.0),
            auth.config.security.trust_proxy_headers,
        )))
    }
}

/// Proxy values must be valid IP addresses. Otherwise retain the trusted peer.
pub(crate) fn resolve(
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    trust: bool,
) -> Option<String> {
    let forwarded = if trust {
        headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<IpAddr>().ok())
            .or_else(|| {
                headers
                    .get("x-forwarded-for")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.rsplit(',').next())
                    .and_then(|v| v.trim().parse::<IpAddr>().ok())
            })
    } else {
        None
    };
    forwarded
        .or_else(|| peer.map(|addr| addr.ip()))
        .map(|ip| ip.to_canonical().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_ip_validation_canonicalization_and_peer_fallback() {
        let peer = Some("198.51.100.4:5000".parse().unwrap());
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", "not-an-ip".parse().unwrap());
        headers.insert(
            "x-forwarded-for",
            "192.0.2.123, 2001:0db8:0:0:0:0:0:1".parse().unwrap(),
        );
        assert_eq!(
            resolve(&headers, peer, false).as_deref(),
            Some("198.51.100.4")
        );
        assert_eq!(
            resolve(&headers, peer, true).as_deref(),
            Some("2001:db8::1")
        );
        headers.insert("x-forwarded-for", "192.0.2.123, invalid".parse().unwrap());
        assert_eq!(
            resolve(&headers, peer, true).as_deref(),
            Some("198.51.100.4")
        );
        headers.insert("x-real-ip", "::ffff:192.0.2.4".parse().unwrap());
        assert_eq!(resolve(&headers, peer, true).as_deref(), Some("192.0.2.4"));
    }
}
