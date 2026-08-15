//! Subscription fetcher port: the I/O boundary for HTTP fetching.
//!
//! The application layer defines this port; the `deve-sub-adapters` crate
//! provides the `HttpFetcher` implementation with SSRF protection, DNS
//! pinning, redirect handling, gzip/deflate, body size limits, and ETag
//! conditional fetch. See `docs/plan/milestones/M4-sources-and-node-pool.md`
//! §"Source refresh flow".

use async_trait::async_trait;
use thiserror::Error;

/// Errors produced by subscription fetching.
#[derive(Debug, Error)]
pub enum FetchError {
    /// The URL was rejected by the SSRF guard.
    #[error("SSRF rejected: {0}")]
    Ssrf(String),

    /// The fetch timed out.
    #[error("fetch timeout after {0}s")]
    Timeout(u64),

    /// The response body exceeded the maximum size.
    #[error("response too large: {0} bytes")]
    TooLarge(u64),

    /// The server returned a non-success HTTP status.
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },

    /// A network or connection error occurred.
    #[error("connection error: {0}")]
    Connection(String),

    /// Too many redirects were encountered.
    #[error("too many redirects")]
    TooManyRedirects,
}

impl FetchError {
    /// Return a redacted representation safe for log output.
    ///
    /// The `Ssrf` variant embeds a URL that may carry credentials; the `Http`
    /// variant embeds up to 1 KiB of the origin's response body. Both must
    /// be stripped before writing to logs (DS-AUD-030, ADR-0007 §"Redaction
    /// boundary").
    #[must_use]
    pub fn redacted(&self) -> String {
        match self {
            Self::Ssrf(_) => "SSRF rejected".to_owned(),
            Self::Http { status, .. } => format!("HTTP {status}"),
            other => other.to_string(),
        }
    }
}

/// The result of a successful fetch.
#[derive(Debug, Clone)]
pub enum FetchResult {
    /// The server returned 200 with a body.
    Ok {
        /// Response body bytes (already decompressed if gzip/deflate).
        body: Vec<u8>,
        /// ETag from the response, if the server provided one.
        etag: Option<String>,
        /// Content-Type from the response, if present.
        content_type: Option<String>,
    },
    /// The server returned 304 Not Modified (the content is unchanged
    /// since the last fetch identified by the ETag).
    NotModified,
}

/// Port for fetching subscription content from a remote URL.
///
/// Implementations must perform SSRF validation before connecting and pin
/// the resolved IP to prevent DNS rebinding (SEC-003).
#[async_trait]
pub trait SubscriptionFetcher: Send + Sync {
    /// Fetch the content at `url`, optionally sending an ETag for
    /// conditional fetch.
    ///
    /// # Errors
    /// - [`FetchError::Ssrf`] — the URL was rejected by the SSRF guard.
    /// - [`FetchError::Timeout`] — the fetch timed out.
    /// - [`FetchError::TooLarge`] — the response body exceeded the size limit.
    /// - [`FetchError::Http`] — the server returned a non-success status.
    /// - [`FetchError::Connection`] — a network error occurred.
    async fn fetch(&self, url: &str, etag: Option<&str>) -> Result<FetchResult, FetchError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DS-AUD-030: `Ssrf` redacts the embedded URL (may carry credentials).
    #[test]
    fn redacted_strips_ssrf_url() {
        let err = FetchError::Ssrf("blocked: http://user:pass@internal.example.com/sub".to_owned());
        let r = err.redacted();
        assert!(!r.contains("user:pass"), "redacted Ssrf must not leak URL");
        assert!(!r.contains("internal.example.com"));
        assert_eq!(r, "SSRF rejected");
    }

    /// DS-AUD-030: `Http` redacts the embedded response body.
    #[test]
    fn redacted_strips_http_body() {
        let err = FetchError::Http {
            status: 500,
            body: "<html>internal server error with secret=abc123</html>".to_owned(),
        };
        let r = err.redacted();
        assert!(
            !r.contains("secret=abc123"),
            "redacted Http must not leak body"
        );
        assert!(!r.contains("<html>"));
        assert_eq!(r, "HTTP 500");
    }

    /// DS-AUD-030: non-sensitive variants pass through unchanged.
    #[test]
    fn redacted_preserves_safe_variants() {
        assert_eq!(
            FetchError::Timeout(30).redacted(),
            "fetch timeout after 30s"
        );
        assert_eq!(
            FetchError::TooLarge(11_000_000).redacted(),
            "response too large: 11000000 bytes"
        );
        assert_eq!(
            FetchError::Connection("dns lookup failed".to_owned()).redacted(),
            "connection error: dns lookup failed"
        );
        assert_eq!(
            FetchError::TooManyRedirects.redacted(),
            "too many redirects"
        );
    }
}
