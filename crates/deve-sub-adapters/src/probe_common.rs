//! Shared utilities for probe source adapters.
//!
//! Extracts the common SSRF-protected HTTP client builder and error-body
//! capping used by [`NezhaProbeAdapter`], `DStatusProbeAdapter`, and
//! `KomariProbeAdapter`. Sensitive fields (`auth_config`,
//! `last_counter_snapshot`) arrive as plaintext in the domain entity;
//! encryption at rest is handled by the storage layer (ADR-0007).
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` §"Probe source
//! adapter Port".

use std::net::{IpAddr, SocketAddr};

use deve_sub_domain::ProbeError;
use url::Url;

use crate::SsrfChecker;

/// Maximum bytes read from an error response body for diagnostics.
///
/// WHY: bounds memory on the non-2xx path so a hostile panel cannot exhaust
/// memory via a large error body, and limits injection of remote content into
/// logs/DB/API responses. Matches `HttpFetcher::ERROR_BODY_CAP`.
pub const ERROR_BODY_CAP: usize = 1024;

/// Default request timeout: 30 seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Read up to [`ERROR_BODY_CAP`] bytes of an error response body.
pub async fn read_error_body(mut response: reqwest::Response) -> String {
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() >= ERROR_BODY_CAP {
            body.truncate(ERROR_BODY_CAP);
            break;
        }
    }
    String::from_utf8_lossy(&body).into_owned()
}

/// Build a `reqwest::Client` with SSRF protection, redirect disabled, and DNS
/// pinning for `url`.
///
/// WHY: prevents an admin-configured endpoint from pointing at internal
/// addresses (loopback, private, link-local, CGNAT) and mitigates DNS
/// rebinding by pinning the resolved IPs. Mirrors `HttpFetcher`'s protection
/// (SEC-001-005).
pub async fn build_ssrf_client(
    ssrf: &dyn SsrfChecker,
    url: &str,
) -> Result<reqwest::Client, ProbeError> {
    let parsed =
        Url::parse(url).map_err(|e| ProbeError::ProbeFailed(format!("invalid URL: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| ProbeError::ProbeFailed("URL has no hostname".to_owned()))?;

    let safe_ips = ssrf
        .check(url)
        .await
        .map_err(|e| ProbeError::ProbeFailed(format!("SSRF check failed: {e}")))?;

    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        // WHY: disable auto-redirect so a compromised panel cannot redirect
        // the server to internal addresses after the SSRF check passes.
        .redirect(reqwest::redirect::Policy::none());

    // WHY: pin DNS to the validated IPs to prevent DNS rebinding between
    // the SSRF check and the actual request. IP literals connect directly
    // and were already validated by the SSRF checker.
    if host.parse::<IpAddr>().is_err() {
        let socket_addrs: Vec<SocketAddr> =
            safe_ips.iter().map(|ip| SocketAddr::new(*ip, 0)).collect();
        builder = builder.resolve_to_addrs(host, &socket_addrs);
    }

    builder
        .build()
        .map_err(|e| ProbeError::ProbeFailed(format!("HTTP client build failed: {e}")))
}
