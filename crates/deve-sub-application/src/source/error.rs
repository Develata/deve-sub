//! Source application errors.

use thiserror::Error;

use super::fetcher::FetchError;
use super::parse::ParseContentError;

/// Errors produced by source application commands and queries.
#[derive(Debug, Error)]
pub enum SourceAppError {
    /// Input validation failed (empty name, invalid URL, etc.).
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// A source was not found.
    #[error("source not found")]
    SourceNotFound,

    /// A node was not found.
    #[error("node not found")]
    NodeNotFound,

    /// A source name is already taken.
    #[error("source name already exists")]
    NameExists,

    /// A source storage operation failed.
    #[error(transparent)]
    Source(#[from] deve_sub_domain::SourceError),

    /// A node chain validation failed (empty, self-reference, duplicate,
    /// missing node, or cycle). See NODE-017 / NODE-018.
    #[error(transparent)]
    NodeChain(#[from] deve_sub_domain::NodeChainError),

    /// A fetch operation failed (SSRF, timeout, HTTP error, etc.).
    ///
    /// Display renders the REDACTED form: subscription URLs routinely embed
    /// tokens in the query string, and the raw `FetchError` text can embed
    /// the full URL (SSRF detail, connection errors) or the origin's error
    /// body — none of which may reach the job table or logs (DS-AUD-030).
    /// `Debug` still carries the full variant for development.
    #[error("fetch failed: {}", .0.redacted())]
    Fetch(#[from] FetchError),

    /// Content parsing failed.
    #[error(transparent)]
    Parse(#[from] ParseContentError),

    /// The refresh yielded zero nodes. The old snapshot is preserved
    /// (SRC-006).
    #[error("refresh yielded zero nodes; old snapshot preserved")]
    ZeroNodes,

    /// The refresh was cancelled by the user or a shutdown signal (B-15).
    /// No snapshot was published.
    #[error("refresh cancelled")]
    Cancelled,

    /// A refresh is already in progress for this source (B-15 lease).
    #[error("refresh already in progress for source {0}")]
    RefreshInProgress(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DS-AUD-030: the Display of a fetch error must not leak the URL
    /// (subscription tokens ride in the query string) or the origin's
    /// error body — both reach the job table and logs via `to_string()`.
    #[test]
    fn fetch_error_display_is_redacted() {
        let err = SourceAppError::Fetch(FetchError::Ssrf(
            "blocked: https://host/api/v1/client/subscribe?token=SECRET".to_owned(),
        ));
        let msg = err.to_string();
        assert!(!msg.contains("SECRET"), "display leaked token: {msg}");
        assert!(!msg.contains("host/api"), "display leaked URL: {msg}");

        let err = SourceAppError::Fetch(FetchError::Http {
            status: 500,
            body: "origin secret diagnostic SECRET".to_owned(),
        });
        let msg = err.to_string();
        assert!(!msg.contains("origin secret"), "display leaked body: {msg}");
        assert!(msg.contains("500"), "status must remain: {msg}");
    }
}
