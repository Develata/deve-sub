//! Domain-level structured errors.
//!
//! Uses `thiserror` per the error-layer policy in
//! `docs/plan/00-engineering-constitution.md` §"Error layer".

use thiserror::Error;

/// Errors produced by domain operations.
#[derive(Debug, Error)]
pub enum DomainError {
    #[error("unsupported protocol: {0}")]
    UnsupportedProtocol(String),

    #[error("invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(String),
}
