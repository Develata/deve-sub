//! Subscription domain errors.

use thiserror::Error;

/// Errors produced by subscription operations.
#[derive(Debug, Error)]
pub enum SubscriptionError {
    /// A subscription was not found.
    #[error("subscription not found")]
    SubscriptionNotFound,

    /// A subscription slug is already taken for the owner.
    #[error("subscription slug already exists for this owner")]
    SlugExists,

    /// A subscription token was not found.
    #[error("subscription token not found")]
    TokenNotFound,

    /// A short code was not found.
    #[error("short code not found")]
    ShortCodeNotFound,

    /// A short code string already exists (UNIQUE constraint violation).
    /// The application layer retries with a fresh CSPRNG code (OUT-013).
    #[error("short code already exists")]
    ShortCodeExists,

    /// A temp link was not found.
    #[error("temp link not found")]
    TempLinkNotFound,

    /// A temp link has been revoked or has expired.
    #[error("temp link revoked or expired")]
    TempLinkInvalid,

    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),
}
