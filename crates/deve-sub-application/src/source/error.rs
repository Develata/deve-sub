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
    #[error(transparent)]
    Fetch(#[from] FetchError),

    /// Content parsing failed.
    #[error(transparent)]
    Parse(#[from] ParseContentError),

    /// The refresh yielded zero nodes. The old snapshot is preserved
    /// (SRC-006).
    #[error("refresh yielded zero nodes; old snapshot preserved")]
    ZeroNodes,
}
