//! Domain-level structured errors.
//!
//! Uses `thiserror` per the error-layer policy in
//! `docs/plan/00-engineering-constitution.md` §"Error layer".

use thiserror::Error;

/// Errors produced by domain operations.
#[derive(Debug, Error)]
pub enum DomainError {
    /// A protocol kind is recognized but not in the P0 typed set.
    #[error("unsupported protocol: {0}")]
    UnsupportedProtocol(String),

    /// A protocol configuration field has an invalid or inconsistent value.
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// An endpoint host or port is invalid.
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(String),
}
