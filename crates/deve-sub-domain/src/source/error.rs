//! Source domain errors.

use thiserror::Error;

/// Errors produced by source operations.
#[derive(Debug, Error)]
pub enum SourceError {
    /// A source was not found.
    #[error("source not found")]
    SourceNotFound,

    /// A source name is already taken.
    #[error("source name already exists")]
    NameExists,

    /// The source URL is invalid.
    #[error("invalid source URL: {0}")]
    InvalidUrl(String),

    /// The source type is not recognized.
    #[error("invalid source type: {0}")]
    InvalidSourceType(String),

    /// A snapshot was not found.
    #[error("snapshot not found")]
    SnapshotNotFound,

    /// A refresh is already in progress for this source.
    #[error("refresh already in progress for source {0}")]
    RefreshInProgress(String),

    /// The fetch was rejected by the SSRF guard.
    #[error("SSRF rejected: {0}")]
    SsrfRejected(String),

    /// The fetch timed out.
    #[error("fetch timeout after {0}s")]
    FetchTimeout(u64),

    /// The response exceeded the maximum size.
    #[error("response too large: {0} bytes")]
    ResponseTooLarge(u64),

    /// The response could not be parsed.
    #[error("parse error: {0}")]
    ParseError(String),

    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),
}
