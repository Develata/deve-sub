//! Kernel-level structured errors.
//!
//! All library crates use `thiserror` for structured errors. See
//! `docs/plan/00-engineering-constitution.md` §"Error layer".

use thiserror::Error;

/// Errors produced by kernel primitives.
#[derive(Debug, Error)]
pub enum KernelError {
    /// A ULID string could not be parsed as an entity identifier.
    ///
    /// `kind` is a lowercase human-readable label (e.g. `"node"`, `"user"`)
    /// used in the error message. This generic variant replaces per-aggregate
    /// `Invalid*Id` variants so that adding a new ID type does not require a
    /// new error variant.
    #[error("invalid {kind} ID: {value}")]
    InvalidId {
        /// Lowercase human-readable label for the ID kind.
        kind: &'static str,
        /// The invalid input string.
        value: String,
    },

    /// A Unix timestamp was out of the representable range.
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
        let e = KernelError::InvalidId {
            kind: "node",
            value: "bad".to_owned(),
        };
        assert_eq!(e.to_string(), "invalid node ID: bad");
    }
}
