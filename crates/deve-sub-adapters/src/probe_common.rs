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

/// Maximum bytes read from a success response body.
///
/// WHY: a compromised or buggy panel could return an enormous JSON body.
/// 1 MiB is generous for node/server lists while preventing unbounded memory
/// growth.
pub const SUCCESS_BODY_CAP: usize = 1024 * 1024;

/// Default request timeout: 30 seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Read a bounded diagnostic prefix. Partial error bodies are diagnostic only.
pub async fn read_error_body(mut response: reqwest::Response) -> String {
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        let count = chunk.len().min(ERROR_BODY_CAP - body.len());
        body.extend_from_slice(&chunk[..count]);
        if body.len() == ERROR_BODY_CAP {
            break;
        }
    }
    String::from_utf8_lossy(&body).into_owned()
}

/// Read a complete bounded success body; never parse a truncated response.
pub async fn read_body_capped(
    mut response: reqwest::Response,
    cap: usize,
) -> Result<String, ProbeError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ProbeError::ProbeFailed("response body read failed".into()))?
    {
        if chunk.len() > cap.saturating_sub(body.len()) {
            return Err(ProbeError::ProbeFailed(format!(
                "response body exceeds {cap} bytes"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body)
        .map_err(|_| ProbeError::ProbeFailed("response body is not UTF-8".into()))
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

    let safe_ips = tokio::time::timeout(std::time::Duration::from_secs(5), ssrf.check(url))
        .await
        .map_err(|_| ProbeError::ProbeFailed("SSRF DNS lookup timed out".into()))?
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

#[cfg(test)]
#[path = "probe_common_tests.rs"]
mod tests;
