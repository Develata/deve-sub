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

    /// A short code was not found.
    #[error("short code not found")]
    ShortCodeNotFound,

    /// A temp link was not found.
    #[error("temp link not found")]
    TempLinkNotFound,

    /// A temp link has been revoked or has expired (delivery: 404, no leak).
    #[error("temp link revoked or expired")]
    TempLinkInvalid,

    /// The referenced template was not found.
    #[error("template not found")]
    TemplateNotFound,

    /// The requested target profile is not recognized.
    #[error("unknown profile: {0}")]
    UnknownProfile(String),

    /// The subscription is disabled. Delivery returns 404 (no existence leak).
    #[error("subscription disabled")]
    SubscriptionDisabled,

    /// The subscription has expired.
    #[error("subscription expired")]
    SubscriptionExpired,

    /// The owning user is disabled or expired.
    #[error("user inactive or expired")]
    UserInactive,

    /// The subscription or user traffic quota is exceeded.
    #[error("traffic quota exceeded")]
    TrafficExceeded,

    /// On-demand generation failed during delivery and no cached content is
    /// available. Delivery returns 503 (constraint #19).
    #[error("generation failed during delivery: {0}")]
    GenerationFailed(String),

    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// A subscription domain or storage operation failed.
    #[error(transparent)]
    Subscription(#[from] SubscriptionError),

    /// An identity storage operation failed (user lookup for delivery).
    #[error(transparent)]
    Identity(#[from] deve_sub_domain::IdentityError),

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
        SubscriptionError::ShortCodeNotFound => SubscriptionAppError::ShortCodeNotFound,
        SubscriptionError::TempLinkNotFound => SubscriptionAppError::TempLinkNotFound,
        SubscriptionError::TempLinkInvalid => SubscriptionAppError::TempLinkInvalid,
        SubscriptionError::ShortCodeExists | SubscriptionError::Storage(_) => {
            SubscriptionAppError::Subscription(e)
        }
    }
}
