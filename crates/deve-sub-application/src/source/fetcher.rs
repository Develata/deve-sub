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
