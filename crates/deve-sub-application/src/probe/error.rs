//! Probe application errors.

use thiserror::Error;

use deve_sub_domain::ProbeError;

/// Errors produced by probe application commands and queries.
#[derive(Debug, Error)]
pub enum ProbeAppError {
    /// Input validation failed (empty name, invalid URL, empty node list,
    /// invalid probe type).
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// A probe source was not found.
    #[error("probe source not found")]
    SourceNotFound,

    /// A probe run was not found.
    #[error("probe run not found")]
    RunNotFound,

    /// A probe source name is already taken.
    #[error("probe source name already exists")]
    NameExists,

    /// A probe run cannot be cancelled because it is already terminal.
    #[error("probe run is already terminal")]
    RunAlreadyTerminal,

    /// A domain-level probe error.
    #[error(transparent)]
    Domain(#[from] ProbeError),
}
