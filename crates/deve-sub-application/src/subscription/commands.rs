//! Subscription application commands: create, update, delete, list, get,
//! rotate token.
//!
//! These functions orchestrate domain services and port interfaces. They do
//! not execute SQL directly. One API operation maps to one command. See
//! `docs/plan/03-architecture.md` §"Lightweight CQRS" and
//! `docs/plan/milestones/M6-subscription-distribution.md`.
//!
//! Token security: the plaintext delivery token is CSPRNG-generated (32 bytes,
//! base64url no padding), stored only as an HMAC-SHA256 digest, and returned
//! exactly once at creation/rotation time. The plaintext is never persisted
//! and never appears in logs. See `docs/plan/00-engineering-constitution.md`
//! §"Data and security".

use deve_sub_compatibility::ProfileKind;
use deve_sub_domain::{
    NodeSelector, ShortCode, ShortCodeRepository, Subscription, SubscriptionRepository,
    SubscriptionToken, SubscriptionTokenRepository, TempLink, TempLinkRepository,
};
use deve_sub_kernel::{SubscriptionId, TempLinkId, TemplateId, Timestamp, UserId};
use deve_sub_security::{MasterKey, generate_session_token, generate_short_code, hmac_digest};
use time::format_description::well_known::Rfc3339;

use super::error::{SubscriptionAppError, map_subscription_error};

/// HMAC purpose for subscription delivery token hashing.
///
/// WHY: domain separation — a digest computed under this purpose cannot be
/// replayed as a session, recovery, or challenge token digest, and vice
/// versa. See `deve-sub-security/src/hmac.rs`.
pub(super) const PURPOSE_SUBSCRIPTION_TOKEN: &str = "subscription_token";

/// Maximum subscription name length.
const MAX_NAME_LEN: usize = 128;

/// Maximum slug length.
const MAX_SLUG_LEN: usize = 128;

/// Default page size for list queries.
const DEFAULT_LIST_LIMIT: u32 = 50;

/// Parse an ISO 8601 (RFC 3339) timestamp string into a [`Timestamp`].
fn parse_iso8601(s: &str) -> Result<Timestamp, SubscriptionAppError> {
    time::OffsetDateTime::parse(s, &Rfc3339)
        .map(Timestamp::from_offset_date_time)
        .map_err(|e| SubscriptionAppError::InvalidInput(format!("invalid expires_at: {e}")))
}

/// Validate a subscription name at the application boundary.
fn validate_name(name: &str) -> Result<(), SubscriptionAppError> {
    if name.is_empty() {
        return Err(SubscriptionAppError::InvalidInput(
            "name must not be empty".to_owned(),
        ));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(SubscriptionAppError::InvalidInput(format!(
            "name must not exceed {MAX_NAME_LEN} characters"
        )));
    }
    Ok(())
}

/// Validate a subscription slug at the application boundary.
fn validate_slug(slug: &str) -> Result<(), SubscriptionAppError> {
    if slug.is_empty() {
        return Err(SubscriptionAppError::InvalidInput(
            "slug must not be empty".to_owned(),
        ));
    }
    if slug.len() > MAX_SLUG_LEN {
        return Err(SubscriptionAppError::InvalidInput(format!(
            "slug must not exceed {MAX_SLUG_LEN} characters"
        )));
    }
    Ok(())
}

/// Validate a profile string and return the parsed [`ProfileKind`].
fn validate_profile(profile: &str) -> Result<ProfileKind, SubscriptionAppError> {
    ProfileKind::from_kebab(profile)
        .ok_or_else(|| SubscriptionAppError::UnknownProfile(profile.to_owned()))
}

/// Parse a [`NodeSelector`] from a raw JSON value.
fn parse_node_selection(value: serde_json::Value) -> Result<NodeSelector, SubscriptionAppError> {
    serde_json::from_value(value)
        .map_err(|e| SubscriptionAppError::InvalidInput(format!("invalid node_selection: {e}")))
}

/// Parameters for [`create_subscription`].
pub struct CreateSubscriptionParams {
    /// Human-readable name.
    pub name: String,
    /// URL-safe slug, unique per owner.
    pub slug: String,
    /// The owning user.
    pub owner_id: UserId,
    /// The template this subscription binds.
    pub template_id: TemplateId,
    /// Target output profile (kebab-case, e.g. `"mihomo"`).
    pub profile: String,
    /// Node selection configuration as raw JSON (parsed into [`NodeSelector`]).
    pub node_selection: serde_json::Value,
    /// Traffic limit in bytes. `None` = unlimited.
    pub traffic_limit: Option<u64>,
    /// Subscription expiry as an ISO 8601 string. `None` = never expires.
    pub expires_at: Option<String>,
}

/// Result of a successful subscription creation.
#[derive(Debug, Clone)]
pub struct CreateSubscriptionResult {
    /// The created subscription aggregate.
    pub subscription: Subscription,
    /// The plaintext delivery token. Shown once; never persisted.
    pub token_plaintext: String,
}

/// Create a new subscription with a freshly generated delivery token.
///
/// Validates name, slug, and profile. Generates a CSPRNG 32-byte token
/// (base64url no padding), stores only the HMAC-SHA256 digest, and returns
/// the plaintext once. The subscription is persisted first, then the token
/// row (the token table has a FK to subscriptions; the subscription table's
/// `token_id` has no FK so insert order is subscriptions-then-tokens).
///
/// # Errors
/// - [`SubscriptionAppError::InvalidInput`] — validation failed.
/// - [`SubscriptionAppError::UnknownProfile`] — profile not recognized.
/// - [`SubscriptionAppError::SlugExists`] — slug collision.
/// - [`SubscriptionAppError::Subscription`] — storage error.
pub async fn create_subscription(
    sub_repo: &dyn SubscriptionRepository,
    master_key: &MasterKey,
    params: CreateSubscriptionParams,
) -> Result<CreateSubscriptionResult, SubscriptionAppError> {
    validate_name(&params.name)?;
    validate_slug(&params.slug)?;
    validate_profile(&params.profile)?;
    let node_selection = parse_node_selection(params.node_selection)?;
    let expires_at = params
        .expires_at
        .as_deref()
        .map(parse_iso8601)
        .transpose()?;

    // WHY: the plaintext token is CSPRNG-generated and returned to the caller
    // exactly once. Only the HMAC-SHA256 digest is persisted. A database
    // compromise alone cannot forge delivery tokens. See SEC-009.
    let token_plaintext = generate_session_token()?;
    let token_digest = hmac_digest(
        PURPOSE_SUBSCRIPTION_TOKEN,
        &token_plaintext,
        master_key.as_bytes(),
    )?;

    let token_id = deve_sub_kernel::SubscriptionTokenId::new();
    let mut subscription = Subscription::new(
        &params.name,
        &params.slug,
        params.owner_id,
        params.template_id,
        &params.profile,
        node_selection,
        token_id,
    );
    subscription.traffic_limit = params.traffic_limit;
    subscription.expires_at = expires_at;

    let token = SubscriptionToken {
        id: token_id,
        subscription_id: subscription.id,
        token_digest,
        previous_token_digest: None,
        rotation_grace_until: None,
        issued_at: Timestamp::now(),
    };

    sub_repo
        .create_with_token(&subscription, &token)
        .await
        .map_err(map_subscription_error)?;

    Ok(CreateSubscriptionResult {
        subscription,
        token_plaintext,
    })
}

/// Parameters for [`update_subscription`].
pub struct UpdateSubscriptionParams {
    /// ID of the subscription to update.
    pub id: SubscriptionId,
    /// New human-readable name.
    pub name: String,
    /// New URL-safe slug.
    pub slug: String,
    /// Pinned template version. `None` follows the template's active version.
    pub template_version_pin: Option<u64>,
    /// New target output profile (kebab-case).
    pub profile: String,
    /// New node selection configuration as raw JSON.
    pub node_selection: serde_json::Value,
    /// New traffic limit in bytes. `None` = unlimited.
    pub traffic_limit: Option<u64>,
    /// New subscription expiry as an ISO 8601 string. `None` = never expires.
    pub expires_at: Option<String>,
    /// Whether delivery is enabled. `None` preserves the current value.
    pub enabled: Option<bool>,
}

/// Update an existing subscription's mutable fields.
///
/// Loads the subscription, validates the new name/slug/profile, updates the
/// mutable fields, and persists. The token is not affected by this command.
///
/// # Errors
/// - [`SubscriptionAppError::SubscriptionNotFound`] — subscription missing.
/// - [`SubscriptionAppError::InvalidInput`] — validation failed.
/// - [`SubscriptionAppError::UnknownProfile`] — profile not recognized.
/// - [`SubscriptionAppError::SlugExists`] — slug collision.
/// - [`SubscriptionAppError::Subscription`] — storage error.
pub async fn update_subscription(
    sub_repo: &dyn SubscriptionRepository,
    params: UpdateSubscriptionParams,
) -> Result<Subscription, SubscriptionAppError> {
    validate_name(&params.name)?;
    validate_slug(&params.slug)?;
    validate_profile(&params.profile)?;
    let node_selection = parse_node_selection(params.node_selection)?;
    let expires_at = params
        .expires_at
        .as_deref()
        .map(parse_iso8601)
        .transpose()?;

    let mut subscription = sub_repo
        .find_by_id(params.id)
        .await
        .map_err(map_subscription_error)?
        .ok_or(SubscriptionAppError::SubscriptionNotFound)?;

    subscription.name = params.name;
    subscription.slug = params.slug;
    subscription.template_version_pin = params.template_version_pin;
    subscription.profile = params.profile;
    subscription.node_selection = node_selection;
    subscription.traffic_limit = params.traffic_limit;
    subscription.expires_at = expires_at;
    if let Some(enabled) = params.enabled {
        subscription.enabled = enabled;
    }
    subscription.updated_at = Timestamp::now();

    sub_repo
        .update(&subscription)
        .await
        .map_err(map_subscription_error)?;

    Ok(subscription)
}

/// Delete a subscription by ID. The token row is cascade-deleted by the
/// database (ON DELETE CASCADE in migration 0009).
///
/// # Errors
/// - [`SubscriptionAppError::SubscriptionNotFound`] — subscription missing.
/// - [`SubscriptionAppError::Subscription`] — storage error.
pub async fn delete_subscription(
    sub_repo: &dyn SubscriptionRepository,
    id: SubscriptionId,
) -> Result<(), SubscriptionAppError> {
    sub_repo.delete(id).await.map_err(map_subscription_error)
}

/// Get a subscription by ID.
///
/// # Errors
/// - [`SubscriptionAppError::Subscription`] — storage error.
pub async fn get_subscription(
    sub_repo: &dyn SubscriptionRepository,
    id: SubscriptionId,
) -> Result<Option<Subscription>, SubscriptionAppError> {
    sub_repo
        .find_by_id(id)
        .await
        .map_err(map_subscription_error)
}

/// List subscriptions for an owner with cursor pagination.
///
/// # Errors
/// - [`SubscriptionAppError::Subscription`] — storage error.
pub async fn list_subscriptions(
    sub_repo: &dyn SubscriptionRepository,
    owner_id: UserId,
    cursor: Option<SubscriptionId>,
    limit: Option<u32>,
) -> Result<Vec<Subscription>, SubscriptionAppError> {
    let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT);
    sub_repo
        .list(owner_id, cursor, limit)
        .await
        .map_err(map_subscription_error)
}

/// Result of a successful token rotation.
#[derive(Debug, Clone)]
pub struct RotateTokenResult {
    /// The token row id (stable across rotations).
    pub token_id: deve_sub_kernel::SubscriptionTokenId,
    /// The new plaintext delivery token. Shown once; never persisted.
    pub token_plaintext: String,
}

/// Rotate a subscription's delivery token.
///
/// Generates a new CSPRNG plaintext token, computes its HMAC-SHA256 digest,
/// and replaces the current token's digest in place. The previous digest is
/// retained in `previous_token_digest` so both old and new tokens remain
/// valid during the grace period (`None` = permanent grace). The plaintext is
/// returned once.
///
/// # Errors
/// - [`SubscriptionAppError::SubscriptionNotFound`] — subscription missing.
/// - [`SubscriptionAppError::TokenNotFound`] — no token row exists.
/// - [`SubscriptionAppError::Subscription`] — storage error.
pub async fn rotate_token(
    sub_repo: &dyn SubscriptionRepository,
    token_repo: &dyn SubscriptionTokenRepository,
    master_key: &MasterKey,
    subscription_id: SubscriptionId,
    grace: Option<time::Duration>,
) -> Result<RotateTokenResult, SubscriptionAppError> {
    // WHY: verify the subscription exists before rotating so a mistyped ULID
    // surfaces as SubscriptionNotFound rather than a TokenNotFound from the
    // token lookup.
    let _subscription = sub_repo
        .find_by_id(subscription_id)
        .await
        .map_err(map_subscription_error)?
        .ok_or(SubscriptionAppError::SubscriptionNotFound)?;

    let token_plaintext = generate_session_token()?;
    let token_digest = hmac_digest(
        PURPOSE_SUBSCRIPTION_TOKEN,
        &token_plaintext,
        master_key.as_bytes(),
    )?;

    let now = Timestamp::now();
    let grace_until = grace.map(|d| now + d);

    let new_token = SubscriptionToken {
        id: deve_sub_kernel::SubscriptionTokenId::new(),
        subscription_id,
        token_digest,
        previous_token_digest: None,
        rotation_grace_until: grace_until,
        issued_at: now,
    };

    let updated = token_repo
        .rotate(subscription_id, &new_token, grace_until)
        .await
        .map_err(map_subscription_error)?;

    Ok(RotateTokenResult {
        token_id: updated.id,
        token_plaintext,
    })
}

/// Maximum retry attempts for short code UNIQUE conflict (OUT-013). After this
/// many collisions (astronomically unlikely with 47 bits of entropy), return
/// a storage error.
const SHORT_CODE_MAX_RETRIES: u32 = 8;

/// Result of a successful short code (re)generation.
#[derive(Debug, Clone)]
pub struct ShortCodeResult {
    /// The short code row id.
    pub short_code_id: deve_sub_kernel::ShortCodeId,
    /// The public base62 short code string (e.g. `"aB3xK9mQ"`).
    pub code: String,
}

/// (Re)generate a short code for a subscription.
///
/// If the subscription already has a short code, the old row is deleted first.
/// Generates a CSPRNG base62 code and retries on UNIQUE conflict (OUT-013).
/// Links the new short code to the subscription via `set_short_code_id`.
///
/// # Errors
/// - [`SubscriptionAppError::SubscriptionNotFound`] — subscription missing.
/// - [`SubscriptionAppError::Subscription`] — storage error after retry budget
///   exhausted.
pub async fn regenerate_short_code(
    sub_repo: &dyn SubscriptionRepository,
    short_code_repo: &dyn ShortCodeRepository,
    subscription_id: SubscriptionId,
) -> Result<ShortCodeResult, SubscriptionAppError> {
    let subscription = sub_repo
        .find_by_id(subscription_id)
        .await
        .map_err(map_subscription_error)?
        .ok_or(SubscriptionAppError::SubscriptionNotFound)?;

    for _ in 0..SHORT_CODE_MAX_RETRIES {
        let code = generate_short_code()?;
        let short_code = ShortCode::new(subscription_id, code.clone());
        match short_code_repo
            .replace(subscription_id, subscription.short_code_id, &short_code)
            .await
        {
            Ok(()) => {
                return Ok(ShortCodeResult {
                    short_code_id: short_code.id,
                    code,
                });
            }
            Err(deve_sub_domain::SubscriptionError::ShortCodeExists) => continue,
            Err(e) => return Err(map_subscription_error(e)),
        }
    }

    Err(SubscriptionAppError::Storage(format!(
        "short code generation exhausted {SHORT_CODE_MAX_RETRIES} retries"
    )))
}

/// Parameters for [`create_temp_link`].
pub struct CreateTempLinkParams {
    /// The subscription this temp link delivers.
    pub subscription_id: SubscriptionId,
    /// When the temp link expires. Delivery returns 404 after this time.
    pub expires_at: Timestamp,
}

/// Result of a successful temp link creation.
#[derive(Debug, Clone)]
pub struct CreateTempLinkResult {
    /// The temp link row id.
    pub temp_link_id: TempLinkId,
    /// The plaintext temp link token. Shown once; never persisted.
    pub token_plaintext: String,
    /// The expiry timestamp.
    pub expires_at: Timestamp,
}

/// Create a temporary delivery link for a subscription.
///
/// Generates a CSPRNG plaintext temp token, stores only the HMAC-SHA256
/// digest, and returns the plaintext once. The temp link is valid until
/// `expires_at` or until revoked via [`revoke_temp_link`].
///
/// # Errors
/// - [`SubscriptionAppError::SubscriptionNotFound`] — subscription missing.
/// - [`SubscriptionAppError::Security`] — token generation or HMAC failed.
/// - [`SubscriptionAppError::Subscription`] — storage error.
pub async fn create_temp_link(
    sub_repo: &dyn SubscriptionRepository,
    temp_link_repo: &dyn TempLinkRepository,
    master_key: &MasterKey,
    params: CreateTempLinkParams,
) -> Result<CreateTempLinkResult, SubscriptionAppError> {
    let _subscription = sub_repo
        .find_by_id(params.subscription_id)
        .await
        .map_err(map_subscription_error)?
        .ok_or(SubscriptionAppError::SubscriptionNotFound)?;

    let token_plaintext = generate_session_token()?;
    let token_digest = hmac_digest(
        PURPOSE_SUBSCRIPTION_TOKEN,
        &token_plaintext,
        master_key.as_bytes(),
    )?;

    let temp_link = TempLink::new(params.subscription_id, token_digest, params.expires_at);
    temp_link_repo
        .create(&temp_link)
        .await
        .map_err(map_subscription_error)?;

    Ok(CreateTempLinkResult {
        temp_link_id: temp_link.id,
        token_plaintext,
        expires_at: temp_link.expires_at,
    })
}

/// Revoke a temporary delivery link.
///
/// Marks the temp link as revoked so subsequent delivery via
/// `GET /sub/{temp_token}` returns 404.
///
/// # Errors
/// - [`SubscriptionAppError::TempLinkNotFound`] — no temp link matches the id.
/// - [`SubscriptionAppError::Subscription`] — storage error.
pub async fn revoke_temp_link(
    temp_link_repo: &dyn TempLinkRepository,
    temp_link_id: TempLinkId,
) -> Result<(), SubscriptionAppError> {
    temp_link_repo
        .revoke(temp_link_id)
        .await
        .map_err(map_subscription_error)
}
