//! Audit log domain errors.

use thiserror::Error;

/// Errors produced by audit log operations.
#[derive(Debug, Error)]
pub enum AuditError {
    /// The cleanup scope or time range is invalid.
    #[error("invalid audit request: {0}")]
    Invalid(String),
    /// Another cleanup changed the previewed batch; nothing was deleted.
    #[error("audit cleanup preview is stale; preview again")]
    Conflict,
    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),
}
