//! Source application errors.

use thiserror::Error;

/// Errors produced by source application commands and queries.
#[derive(Debug, Error)]
pub enum SourceAppError {
    /// Input validation failed (empty name, invalid URL, etc.).
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// A source was not found.
    #[error("source not found")]
    SourceNotFound,

    /// A source name is already taken.
    #[error("source name already exists")]
    NameExists,

    /// A source storage operation failed.
    #[error(transparent)]
    Source(#[from] deve_sub_domain::SourceError),
}
