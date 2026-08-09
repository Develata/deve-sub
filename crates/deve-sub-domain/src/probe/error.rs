//! Probe domain errors.

use thiserror::Error;

/// Errors produced by probe operations.
#[derive(Debug, Error)]
pub enum ProbeError {
    /// A probe source was not found.
    #[error("probe source not found")]
    SourceNotFound,

    /// A probe run was not found.
    #[error("probe run not found")]
    RunNotFound,

    /// A probe source name is already taken.
    #[error("probe source name already exists")]
    NameExists,

    /// Invalid input was provided (e.g. malformed endpoint URL, empty node
    /// list, invalid probe type).
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// A probe run cannot be cancelled because it is already terminal
    /// (completed, cancelled, or failed).
    #[error("probe run is already terminal")]
    RunAlreadyTerminal,

    /// A latency probe failed (network error, timeout, handshake failure).
    /// Carries the error class for classification.
    #[error("probe failed: {0}")]
    ProbeFailed(String),

    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),
}
