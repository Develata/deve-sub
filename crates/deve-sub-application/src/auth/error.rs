//! Auth application errors.

use thiserror::Error;

/// Errors produced by auth application commands.
#[derive(Debug, Error)]
pub enum AuthError {
    /// Admin setup was attempted but users already exist.
    #[error("admin already initialized")]
    AlreadyInitialized,

    /// Login credentials were invalid (wrong password, unknown user, or
    /// disabled account). The same error is returned for all three cases
    /// to avoid leaking whether a username exists (AUTH-003).
    #[error("invalid credentials")]
    InvalidCredentials,

    /// Input validation failed (empty username, password too short, etc.).
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// Too many failed login attempts. The account or IP is temporarily
    /// locked (AUTH-004).
    #[error("rate limited")]
    RateLimited,

    /// A security primitive failed.
    #[error(transparent)]
    Security(#[from] deve_sub_security::SecurityError),

    /// An identity storage operation failed.
    #[error(transparent)]
    Identity(#[from] deve_sub_domain::IdentityError),
}
