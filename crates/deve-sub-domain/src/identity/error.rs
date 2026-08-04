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

    /// 2FA is already enabled for this user.
    #[error("2FA already enabled")]
    TwoFactorAlreadyEnabled,

    /// 2FA is not enabled for this user.
    #[error("2FA not enabled")]
    TwoFactorNotEnabled,

    /// No TOTP secret found for the user (setup not completed).
    #[error("TOTP secret not found")]
    TotpSecretNotFound,

    /// A recovery code was not found or has already been used.
    #[error("recovery code not found or already used")]
    RecoveryCodeNotFound,

    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),
}
