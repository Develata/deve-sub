//! Subscription application errors.

use thiserror::Error;

use deve_sub_domain::SubscriptionError;

/// Errors produced by subscription application commands and queries.
#[derive(Debug, Error)]
pub enum SubscriptionAppError {
    /// Input validation failed (empty name/slug, invalid profile, bad JSON,
    /// etc.).
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// A subscription was not found.
    #[error("subscription not found")]
    SubscriptionNotFound,

    /// A subscription slug is already taken for the owner.
    #[error("subscription slug already exists for this owner")]
    SlugExists,

    /// A subscription token was not found.
    #[error("subscription token not found")]
    TokenNotFound,

    /// The referenced template was not found.
    #[error("template not found")]
    TemplateNotFound,

    /// The requested target profile is not recognized.
    #[error("unknown profile: {0}")]
    UnknownProfile(String),

    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// A subscription domain or storage operation failed.
    #[error(transparent)]
    Subscription(#[from] SubscriptionError),

    /// A cryptographic or token operation failed.
    #[error(transparent)]
    Security(#[from] deve_sub_security::SecurityError),
}

/// Map a [`SubscriptionError`] to the matching [`SubscriptionAppError`]
/// variant, so domain-level not-found / slug-conflict surface as the
/// application-level variant rather than the wrapped `Subscription(…)`.
pub(super) fn map_subscription_error(e: SubscriptionError) -> SubscriptionAppError {
    match e {
        SubscriptionError::SubscriptionNotFound => SubscriptionAppError::SubscriptionNotFound,
        SubscriptionError::SlugExists => SubscriptionAppError::SlugExists,
        SubscriptionError::TokenNotFound => SubscriptionAppError::TokenNotFound,
        other => SubscriptionAppError::Subscription(other),
    }
}
