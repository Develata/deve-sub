//! Kernel-level structured errors.
//!
//! All library crates use `thiserror` for structured errors. See
//! `docs/plan/00-engineering-constitution.md` §"Error layer".

use thiserror::Error;

/// Errors produced by kernel primitives.
#[derive(Debug, Error)]
pub enum KernelError {
    /// A ULID string could not be parsed as a [`NodeId`](crate::NodeId).
    #[error("invalid node ID: {0}")]
    InvalidNodeId(String),

    /// A ULID string could not be parsed as a [`TagId`](crate::TagId).
    #[error("invalid tag ID: {0}")]
    InvalidTagId(String),

    /// A ULID string could not be parsed as a [`UserId`](crate::UserId).
    #[error("invalid user ID: {0}")]
    InvalidUserId(String),

    /// A ULID string could not be parsed as a [`SessionId`](crate::SessionId).
    #[error("invalid session ID: {0}")]
    InvalidSessionId(String),

    /// A ULID string could not be parsed as a [`AuditLogId`](crate::AuditLogId).
    #[error("invalid audit log ID: {0}")]
    InvalidAuditLogId(String),

    /// A ULID string could not be parsed as an [`OutboxEventId`](crate::OutboxEventId).
    #[error("invalid outbox event ID: {0}")]
    InvalidOutboxEventId(String),

    /// An opaque pagination cursor string was malformed.
    #[error("invalid cursor")]
    InvalidCursor,

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
        let e = KernelError::InvalidNodeId("bad".to_owned());
        assert_eq!(e.to_string(), "invalid node ID: bad");
    }
}
