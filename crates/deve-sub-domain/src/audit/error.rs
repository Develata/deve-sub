//! Audit log domain errors.

use thiserror::Error;

/// Errors produced by audit log operations.
#[derive(Debug, Error)]
pub enum AuditError {
    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),
}
