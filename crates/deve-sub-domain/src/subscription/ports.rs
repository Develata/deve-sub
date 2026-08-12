//! Port traits for subscription, subscription token, short code, and temp link
//! storage.

use async_trait::async_trait;

use deve_sub_kernel::{ShortCodeId, SubscriptionId, SubscriptionTokenId, TempLinkId, UserId};

use super::error::SubscriptionError;
use super::{
    ShortCode, Subscription, SubscriptionToken, TempLink, TrafficDailySnapshot, TrafficRecord,
    TrafficSummary,
};

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

    /// Set the `short_code_id` reference on a subscription. Called after a
    /// short code row is inserted so the subscription points to it. Pass
    /// `None` to clear the reference (e.g. after short code deletion).
    async fn set_short_code_id(
        &self,
        subscription_id: SubscriptionId,
        short_code_id: Option<ShortCodeId>,
    ) -> Result<(), SubscriptionError>;
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

    /// Find a token row whose `previous_token_digest` matches the given hash.
    /// Used by delivery to resolve the old token during a rotation grace
    /// period. Returns `None` if no row retains that digest as its previous.
    async fn find_by_previous_token_hash(
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

    /// Clear expired grace tokens: for every token row whose
    /// `rotation_grace_until` is in the past (non-`None` and `<= now`),
    /// set `previous_token_digest = NULL` and `rotation_grace_until = NULL`.
    /// Rows with `None` grace (permanent) are left untouched. Returns the
    /// number of rows cleaned. Called by the grace cleanup scheduler
    /// (constraint #20).
    async fn clear_expired_grace_tokens(
        &self,
        now: deve_sub_kernel::Timestamp,
    ) -> Result<u64, SubscriptionError>;
}

/// Storage boundary for subscription short codes.
///
/// Short codes are CSPRNG-generated base62 strings stored in the clear (they
/// are public lookup keys, not secrets). The `code` column has a UNIQUE
/// constraint for atomic conflict rejection (OUT-013). See
/// `docs/plan/milestones/M6-subscription-distribution.md` §"Token and
/// short-code security model".
#[async_trait]
pub trait ShortCodeRepository: Send + Sync {
    /// Insert a new short code row. Returns
    /// [`SubscriptionError::ShortCodeExists`] on UNIQUE constraint violation
    /// (OUT-013); the application layer retries with a fresh CSPRNG code.
    async fn create(&self, short_code: &ShortCode) -> Result<(), SubscriptionError>;

    /// Find a short code by its code string. Used by the `GET /s/{code}`
    /// delivery handler.
    async fn find_by_code(&self, code: &str) -> Result<Option<ShortCode>, SubscriptionError>;

    /// Find the short code for a subscription, if one exists.
    async fn find_by_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<Option<ShortCode>, SubscriptionError>;

    /// Delete a short code by ID.
    async fn delete(&self, id: ShortCodeId) -> Result<(), SubscriptionError>;

    /// Delete all short codes for a subscription. Called on subscription
    /// delete.
    async fn delete_for_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<(), SubscriptionError>;
}

/// Storage boundary for subscription temporary delivery links.
///
/// Temp link tokens are CSPRNG-generated and stored as HMAC-SHA256 digests,
/// like permanent delivery tokens. Each temp link has a mandatory expiry and a
/// revocation flag. See `docs/plan/milestones/M6-subscription-distribution.md`
/// §"Slicing" Slice 3.
#[async_trait]
pub trait TempLinkRepository: Send + Sync {
    /// Insert a new temp link row.
    async fn create(&self, temp_link: &TempLink) -> Result<(), SubscriptionError>;

    /// Find a temp link by its token digest. Used by the delivery handler to
    /// resolve a `GET /sub/{temp_token}` request.
    async fn find_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<TempLink>, SubscriptionError>;

    /// Mark a temp link as revoked. Returns
    /// [`SubscriptionError::TempLinkNotFound`] if no row matches.
    async fn revoke(&self, id: TempLinkId) -> Result<(), SubscriptionError>;

    /// Delete all temp links for a subscription. Called on subscription
    /// delete.
    async fn delete_for_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<(), SubscriptionError>;

    /// Find a temp link by ID.
    async fn find_by_id(&self, id: TempLinkId) -> Result<Option<TempLink>, SubscriptionError>;
}

/// Storage boundary for subscription traffic accounting records.
///
/// Records are summed per subscription to compute consumed traffic for quota
/// enforcement and the `subscription-userinfo` header. M6 does not infer
/// real proxy traffic from download counts (terminology §116-121). See
/// `docs/plan/milestones/M6-subscription-distribution.md` §"Traffic and
/// expiry policy framework".
#[async_trait]
pub trait TrafficRepository: Send + Sync {
    /// Insert a new traffic record.
    async fn create(&self, record: &TrafficRecord) -> Result<(), SubscriptionError>;

    /// Sum all traffic records for a subscription, returning the aggregate
    /// upload/download totals and a per-source-kind breakdown.
    async fn get_summary(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<TrafficSummary, SubscriptionError>;

    /// Sum traffic records for a subscription within a timestamp range
    /// `[start_iso, end_iso)`, returning the aggregate upload/download totals
    /// and a per-source-kind breakdown. Used by the M10 daily snapshot
    /// aggregation job.
    async fn get_summary_in_range(
        &self,
        subscription_id: SubscriptionId,
        start_iso: &str,
        end_iso: &str,
    ) -> Result<TrafficSummary, SubscriptionError>;

    /// Sum all traffic records across all subscriptions owned by a user,
    /// returning the aggregate upload/download totals. Used for user-level
    /// `traffic_quota` enforcement at delivery time (OUT-011).
    async fn get_summary_for_user(
        &self,
        user_id: UserId,
    ) -> Result<TrafficSummary, SubscriptionError>;

    /// Sum all traffic records across all subscriptions, returning the
    /// aggregate upload/download totals and a per-source-kind breakdown.
    /// Used by the admin dashboard traffic view.
    async fn get_global_summary(&self) -> Result<TrafficSummary, SubscriptionError>;

    /// Delete all traffic records for a subscription. Called on subscription
    /// delete.
    async fn delete_for_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<(), SubscriptionError>;

    /// Sum probe-source traffic records grouped by `(subscription_id,
    /// source_ref_prefix)`, where `source_ref_prefix` is the substring of
    /// `source_ref` before the first `:` (e.g. `nezha`, `dstatus`, `komari`).
    /// Used by the dashboard per-probe-source breakdown (PROBE-005).
    async fn get_probe_traffic_attributions(
        &self,
    ) -> Result<Vec<(SubscriptionId, String, u64, u64)>, SubscriptionError>;

    /// Return the distinct subscription IDs that have traffic records in the
    /// given date range. Used by the M10 aggregation job to know which
    /// subscriptions need snapshot computation.
    async fn subscriptions_with_traffic_in_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<SubscriptionId>, SubscriptionError>;
}

/// Storage boundary for daily traffic snapshots (M10).
///
/// The M10 aggregation job upserts one row per `(subscription_id, date)`.
/// The history query reads snapshots by subscription and date range, or
/// globally across all subscriptions. See
/// `docs/plan/milestones/M10-observability-and-audit.md` §"Traffic daily
/// snapshot model".
#[async_trait]
pub trait TrafficDailySnapshotRepository: Send + Sync {
    /// Upsert a daily snapshot. If a row for `(subscription_id, date)`
    /// already exists, it is replaced.
    async fn upsert(&self, snapshot: &TrafficDailySnapshot) -> Result<(), SubscriptionError>;

    /// List daily snapshots for a subscription within a date range
    /// (inclusive), ordered by date ascending.
    async fn list_for_subscription(
        &self,
        subscription_id: SubscriptionId,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<TrafficDailySnapshot>, SubscriptionError>;

    /// List daily snapshots across all subscriptions within a date range
    /// (inclusive), ordered by date ascending. Used by the global dashboard
    /// history view.
    async fn list_global(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<TrafficDailySnapshot>, SubscriptionError>;
}
