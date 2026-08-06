//! HTTP fetcher adapter with SSRF protection.
//!
//! Implements [`SubscriptionFetcher`] using `reqwest` with:
//! - SSRF guard: rejects localhost, private, link-local, multicast, CGNAT,
//!   IPv6 ULA, and IPv4-mapped IPv6 (SEC-001, SEC-002).
//! - DNS pinning: pins the resolved IP so reqwest does not re-resolve,
//!   preventing DNS rebinding (SEC-003).
//! - Manual redirect handling: follows up to `max_redirects` redirects,
//!   re-checking SSRF on each hop (SEC-004).
//! - Body size limit: rejects responses exceeding `max_body_size`
//!   (SRC-007, SEC-005).
//! - Timeout: rejects slow responses (SRC-008).
//! - gzip/deflate: automatic decompression (SRC-012).
//! - ETag/If-None-Match: conditional fetch, 304 → NotModified.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use async_trait::async_trait;
use deve_sub_application::{FetchError, FetchResult, SubscriptionFetcher};
use deve_sub_security::SsrfGuard;
use url::Url;

/// Default response body size limit: 10 MiB.
const DEFAULT_MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

/// Default request timeout: 30 seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Default maximum redirect hops.
const DEFAULT_MAX_REDIRECTS: usize = 3;

/// Maximum bytes read from an error response body for diagnostics.
///
/// WHY: the success path enforces `max_body_size` via [`Self::read_body`], but
/// the error path must also be bounded to prevent a hostile origin from
/// exhausting memory with a large error-status body (SEC-005 / SRC-007). The
/// body is only used for the `FetchError::Http` diagnostic message and is
/// logged at `warn` level, so a short prefix suffices and limits info-leak.
const ERROR_BODY_CAP: usize = 1024;

/// HTTP fetcher with SSRF protection, DNS pinning, and body size limits.
pub struct HttpFetcher {
    timeout: Duration,
    max_body_size: usize,
    max_redirects: usize,
    user_agent: String,
}

impl HttpFetcher {
    /// Create a new fetcher with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            user_agent: "deve-sub/0.1".to_owned(),
        }
    }

    /// Set the request timeout.
    #[must_use]
    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout = Duration::from_secs(secs);
        self
    }

    /// Set the maximum response body size in bytes.
    #[must_use]
    pub fn max_body_size(mut self, bytes: usize) -> Self {
        self.max_body_size = bytes;
        self
    }

    /// Set the maximum number of redirect hops.
    #[must_use]
    pub fn max_redirects(mut self, hops: usize) -> Self {
        self.max_redirects = hops;
        self
    }

    /// Set the User-Agent string.
    #[must_use]
    pub fn user_agent(mut self, ua: &str) -> Self {
        self.user_agent = ua.to_owned();
        self
    }

    /// Fetch a single URL (no redirect handling). Performs SSRF check and
    /// DNS pinning, then sends the request.
    async fn fetch_single(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> Result<reqwest::Response, FetchError> {
        let parsed =
            Url::parse(url).map_err(|e| FetchError::Connection(format!("invalid URL: {e}")))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| FetchError::Connection("URL has no hostname".to_owned()))?;

        let safe_ips = SsrfGuard::check(url)
            .await
            .map_err(|e| FetchError::Ssrf(e.to_string()))?;

        let mut builder = reqwest::Client::builder()
            .timeout(self.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .gzip(true)
            .deflate(true)
            .user_agent(self.user_agent.clone());

        // WHY: pin DNS only for domain names. IP literals connect directly
        // and have already been validated by SsrfGuard::check.
        if host.parse::<IpAddr>().is_err() {
            let socket_addrs: Vec<SocketAddr> =
                safe_ips.iter().map(|ip| SocketAddr::new(*ip, 0)).collect();
            builder = builder.resolve_to_addrs(host, &socket_addrs);
        }

        let client = builder
            .build()
            .map_err(|e| FetchError::Connection(e.to_string()))?;

        let mut request = client.get(url);
        if let Some(etag) = etag {
            request = request.header("If-None-Match", etag);
        }

        request.send().await.map_err(|e| self.map_error(e))
    }

    /// Read the response body with a size limit.
    async fn read_body(&self, mut response: reqwest::Response) -> Result<Vec<u8>, FetchError> {
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| self.map_error(e))? {
            body.extend_from_slice(&chunk);
            if body.len() > self.max_body_size {
                return Err(FetchError::TooLarge(body.len() as u64));
            }
        }
        Ok(body)
    }

    /// Read up to [`ERROR_BODY_CAP`] bytes of an error response body.
    ///
    /// WHY: bounds memory on the non-200 path so a hostile origin cannot
    /// exhaust memory via a large error-status body.
    async fn read_error_body(&self, mut response: reqwest::Response) -> String {
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

    /// Map a `reqwest::Error` to a `FetchError`.
    fn map_error(&self, e: reqwest::Error) -> FetchError {
        if e.is_timeout() {
            FetchError::Timeout(self.timeout.as_secs())
        } else {
            FetchError::Connection(e.to_string())
        }
    }
}

impl Default for HttpFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SubscriptionFetcher for HttpFetcher {
    async fn fetch(&self, url: &str, etag: Option<&str>) -> Result<FetchResult, FetchError> {
        let mut current_url = url.to_owned();
        let mut current_etag = etag.map(str::to_owned);

        for _ in 0..=self.max_redirects {
            let response = self
                .fetch_single(&current_url, current_etag.as_deref())
                .await?;

            let status = response.status();
            let etag_header = response
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);

            if status == reqwest::StatusCode::OK {
                let body = self.read_body(response).await?;
                return Ok(FetchResult::Ok {
                    body,
                    etag: etag_header,
                    content_type,
                });
            }

            if status == reqwest::StatusCode::NOT_MODIFIED {
                return Ok(FetchResult::NotModified);
            }

            if status.is_redirection() {
                // WHY: clear ETag on redirect — the ETag is for the original
                // resource, not the redirect target.
                current_etag = None;
                let loc = location.ok_or_else(|| {
                    FetchError::Connection("redirect without Location header".to_owned())
                })?;
                let base = Url::parse(&current_url)
                    .map_err(|e| FetchError::Connection(format!("invalid redirect URL: {e}")))?;
                current_url = base
                    .join(&loc)
                    .map_err(|e| FetchError::Connection(format!("invalid Location: {e}")))?
                    .to_string();
                continue;
            }

            let body = self.read_error_body(response).await;
            return Err(FetchError::Http {
                status: status.as_u16(),
                body,
            });
        }

        Err(FetchError::TooManyRedirects)
    }
}
