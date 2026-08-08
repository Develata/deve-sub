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

    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),
}
