//! Port traits for subscription and subscription token storage.

use async_trait::async_trait;

use deve_sub_kernel::{SubscriptionId, SubscriptionTokenId, UserId};

use super::error::SubscriptionError;
use super::{Subscription, SubscriptionToken};

/// Storage boundary for subscription aggregates.
#[async_trait]
pub trait SubscriptionRepository: Send + Sync {
    /// Create a new subscription. Returns
    /// [`SubscriptionError::SlugExists`] if the slug is already taken for the
    /// owner.
    async fn create(&self, subscription: &Subscription) -> Result<(), SubscriptionError>;

    /// Find a subscription by ID.
    async fn find_by_id(
        &self,
        id: SubscriptionId,
    ) -> Result<Option<Subscription>, SubscriptionError>;

    /// Find a subscription by owner and slug. The slug is unique per owner.
    async fn find_by_slug(
        &self,
        owner_id: UserId,
        slug: &str,
    ) -> Result<Option<Subscription>, SubscriptionError>;

    /// List subscriptions for an owner with cursor pagination. Returns up to
    /// `limit` subscriptions whose ULID is strictly greater than `cursor`,
    /// ordered by `id`.
    async fn list(
        &self,
        owner_id: UserId,
        cursor: Option<SubscriptionId>,
        limit: u32,
    ) -> Result<Vec<Subscription>, SubscriptionError>;

    /// Update a subscription's mutable fields (name, slug, template pin,
    /// profile, node selection, traffic limit, expiry, enabled). Returns
    /// [`SubscriptionError::SubscriptionNotFound`] if the subscription does
    /// not exist, or [`SubscriptionError::SlugExists`] on slug collision.
    async fn update(&self, subscription: &Subscription) -> Result<(), SubscriptionError>;

    /// Delete a subscription and its token.
    async fn delete(&self, id: SubscriptionId) -> Result<(), SubscriptionError>;
}

/// Storage boundary for subscription delivery tokens.
///
/// Tokens are stored as HMAC-SHA256 digests; the plaintext is never persisted.
/// See `docs/plan/00-engineering-constitution.md` §"Data and security".
#[async_trait]
pub trait SubscriptionTokenRepository: Send + Sync {
    /// Create a new token row.
    async fn create(&self, token: &SubscriptionToken) -> Result<(), SubscriptionError>;

    /// Find a token by its digest. Used by the delivery handler to resolve
    /// a `/sub/{token}` request.
    async fn find_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<SubscriptionToken>, SubscriptionError>;

    /// Find the active token for a subscription.
    async fn find_active_for_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<Option<SubscriptionToken>, SubscriptionError>;

    /// Rotate a subscription's token: replace the current token with
    /// `new_token`, retaining the previous digest in `previous_token_digest`
    /// with the given grace expiry. Returns the updated token row.
    async fn rotate(
        &self,
        subscription_id: SubscriptionId,
        new_token: &SubscriptionToken,
        grace_until: Option<deve_sub_kernel::Timestamp>,
    ) -> Result<SubscriptionToken, SubscriptionError>;

    /// Delete all tokens for a subscription. Called on subscription delete.
    async fn delete_for_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<(), SubscriptionError>;

    /// Find a token by ID.
    async fn find_by_id(
        &self,
        id: SubscriptionTokenId,
    ) -> Result<Option<SubscriptionToken>, SubscriptionError>;
}
