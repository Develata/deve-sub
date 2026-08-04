//! Identity domain errors.

use thiserror::Error;

/// Errors produced by identity operations.
#[derive(Debug, Error)]
pub enum IdentityError {
    /// A user was not found.
    #[error("user not found")]
    UserNotFound,

    /// A username is already taken.
    #[error("username already exists")]
    UsernameExists,

    /// Users already exist (first-admin gate refused).
    #[error("admin already initialized")]
    AlreadyInitialized,

    /// A session was not found.
    #[error("session not found")]
    SessionNotFound,

    /// A role string did not match a known role.
    #[error("invalid role: {0}")]
    InvalidRole(String),

    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),
}
