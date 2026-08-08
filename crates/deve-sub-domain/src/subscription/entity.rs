//! Subscription aggregate and delivery token entity.
//!
//! A `Subscription` is an independent aggregate root: it binds one
//! `SubscriptionTemplate` (by id, optionally pinned to a specific version),
//! carries its own node-selection configuration, and owns its delivery
//! configuration (token, traffic limit, expiry). Template updates never
//! silently mutate an existing Subscription's selection snapshot; the
//! Subscription is regenerated on demand at delivery time.
//!
//! A `SubscriptionToken` carries the HMAC-SHA256 digest of the CSPRNG-generated
//! plaintext token. The plaintext is returned once at creation/rotation time
//! and never persisted. Token lookup at delivery is by digest. See
//! `docs/plan/00-engineering-constitution.md` §"Data and security" and
//! `docs/plan/milestones/M6-subscription-distribution.md`.

use deve_sub_kernel::{SubscriptionId, SubscriptionTokenId, TemplateId, Timestamp, UserId};

use crate::template::NodeSelector;

/// The subscription aggregate root.
///
/// Represents a long-term subscription distribution entry owned by a user,
/// bound to a template, with its own delivery token, traffic limit, and
/// expiry. Delivery serves the cached generation for
/// `(template_id, template_version, profile)`; on cache miss the M5 generation
/// pipeline runs on demand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    /// Unique identifier (ULID).
    pub id: SubscriptionId,
    /// Human-readable name.
    pub name: String,
    /// URL-safe slug, unique per owner.
    pub slug: String,
    /// The owning user.
    pub owner_id: UserId,
    /// The template this subscription binds.
    pub template_id: TemplateId,
    /// Pinned template version. `None` follows the template's active version.
    pub template_version_pin: Option<u64>,
    /// Target output profile (kebab-case, e.g. `"mihomo"`, `"sing-box"`).
    /// Validated by the application layer via `ProfileKind::from_kebab`.
    pub profile: String,
    /// Node selection configuration (dynamic filters or fixed nodeIds +
    /// revision). Owned by the subscription; template updates do not mutate
    /// it.
    pub node_selection: NodeSelector,
    /// Traffic limit in bytes. `None` = unlimited. Enforced at delivery.
    pub traffic_limit: Option<u64>,
    /// Subscription expiry. `None` = never expires. Enforced at delivery.
    pub expires_at: Option<Timestamp>,
    /// The active delivery token row. Rotation retains the previous token
    /// digest during the grace period.
    pub token_id: SubscriptionTokenId,
    /// Whether delivery is enabled. Disabled subscriptions return 404 at
    /// delivery (no existence leak).
    pub enabled: bool,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last update time.
    pub updated_at: Timestamp,
}

impl Subscription {
    /// Create a new enabled subscription shell. The token row is committed
    /// separately by the application layer via
    /// [`super::ports::SubscriptionTokenRepository`].
    #[must_use]
    pub fn new(
        name: &str,
        slug: &str,
        owner_id: UserId,
        template_id: TemplateId,
        profile: &str,
        node_selection: NodeSelector,
        token_id: SubscriptionTokenId,
    ) -> Self {
        let now = Timestamp::now();
        Self {
            id: SubscriptionId::new(),
            name: name.to_owned(),
            slug: slug.to_owned(),
            owner_id,
            template_id,
            template_version_pin: None,
            profile: profile.to_owned(),
            node_selection,
            traffic_limit: None,
            expires_at: None,
            token_id,
            enabled: true,
            created_at: now,
            updated_at: now,
        }
    }
}

/// A delivery token row for a subscription.
///
/// Stores only the HMAC-SHA256 digest of the CSPRNG-generated plaintext
/// token. The plaintext is returned once at creation/rotation time and never
/// persisted. During rotation grace, `previous_token_digest` holds the prior
/// digest and `rotation_grace_until` marks the expiry (`None` = permanent
/// grace).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionToken {
    /// Unique identifier (ULID). Identifies the row, not the token secret.
    pub id: SubscriptionTokenId,
    /// The subscription this token delivers.
    pub subscription_id: SubscriptionId,
    /// HMAC-SHA256 digest of the plaintext token, base64url-encoded.
    pub token_digest: String,
    /// Previous token digest retained during rotation grace. `None` outside
    /// a grace window.
    pub previous_token_digest: Option<String>,
    /// When the previous token digest expires. `None` = permanent grace.
    pub rotation_grace_until: Option<Timestamp>,
    /// When this token was issued.
    pub issued_at: Timestamp,
}

impl SubscriptionToken {
    /// Create a new token row for a subscription with the given digest.
    #[must_use]
    pub fn new(subscription_id: SubscriptionId, token_digest: String) -> Self {
        Self {
            id: SubscriptionTokenId::new(),
            subscription_id,
            token_digest,
            previous_token_digest: None,
            rotation_grace_until: None,
            issued_at: Timestamp::now(),
        }
    }

    /// Whether a previous token digest is still within its grace window at
    /// the given reference time.
    #[must_use]
    pub fn is_previous_token_valid_at(&self, now: Timestamp) -> bool {
        match (
            self.previous_token_digest.as_ref(),
            self.rotation_grace_until,
        ) {
            (Some(_), None) => true,
            (Some(_), Some(until)) => until > now,
            (None, _) => false,
        }
    }
}
