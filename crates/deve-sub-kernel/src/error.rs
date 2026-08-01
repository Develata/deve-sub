//! Kernel-level structured errors.
//!
//! All library crates use `thiserror` for structured errors. See
//! `docs/plan/00-engineering-constitution.md` §"Error layer".

use thiserror::Error;

/// Errors produced by kernel primitives.
#[derive(Debug, Error)]
pub enum KernelError {
    #[error("invalid node ID: {0}")]
    InvalidNodeId(String),

    #[error("invalid tag ID: {0}")]
    InvalidTagId(String),

    #[error("invalid cursor")]
    InvalidCursor,

    #[error("invalid timestamp")]
    InvalidTimestamp,
}

/// Convenience alias used across the kernel crate.
pub type Result<T> = std::result::Result<T, KernelError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let e = KernelError::InvalidNodeId("bad".to_owned());
        assert_eq!(e.to_string(), "invalid node ID: bad");
    }
}
