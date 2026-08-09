//! Subscription DTOs for the `/api/v1/subscriptions` endpoints.
//!
//! These DTOs are the wire format for subscription lifecycle management.
//! They are owned by the contract crate per ADR-0004: DTOs and `ToSchema`
//! derives live here, not in the API crate. The `node_selection` field is
//! passed as `serde_json::Value` so the contract does not depend on the
//! domain crate; the application layer parses it into a typed `NodeSelector`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Request body for `POST /api/v1/subscriptions`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateSubscriptionRequest {
    /// Human-readable subscription name.
    pub name: String,
    /// URL-safe slug, unique per owner.
    pub slug: String,
    /// The template ULID this subscription binds.
    pub template_id: String,
    /// Target output profile (kebab-case: `mihomo`, `sing-box`, `xray`,
    /// `v2ray`, `shadowrocket`, `uri_list`).
    pub profile: String,
    /// Node selection configuration as a JSON object matching the V3
    /// `nodeSelector` schema (`mode`, `filters`, `nodeIds`, `nodeRevision`).
    pub node_selection: serde_json::Value,
    /// Traffic limit in bytes. `null` = unlimited. Enforced at delivery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traffic_limit: Option<u64>,
    /// Subscription expiry as an ISO 8601 string. `null` = never expires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Request body for `PUT /api/v1/subscriptions/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateSubscriptionRequest {
    /// Human-readable subscription name.
    pub name: String,
    /// URL-safe slug, unique per owner.
    pub slug: String,
    /// Pinned template version. `null` follows the template's active version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_version_pin: Option<u64>,
    /// Target output profile (kebab-case).
    pub profile: String,
    /// Node selection configuration as a JSON object.
    pub node_selection: serde_json::Value,
    /// Traffic limit in bytes. `null` = unlimited.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traffic_limit: Option<u64>,
    /// Subscription expiry as an ISO 8601 string. `null` = never expires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// Whether delivery is enabled. `null` preserves the current value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Subscription information returned by subscription management endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubscriptionDto {
    /// ULID identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// URL-safe slug, unique per owner.
    pub slug: String,
    /// The owning user ULID.
    pub owner_id: String,
    /// The bound template ULID.
    pub template_id: String,
    /// Pinned template version. `null` follows the template's active version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_version_pin: Option<u64>,
    /// Target output profile (kebab-case).
    pub profile: String,
    /// Node selection configuration as a JSON object.
    pub node_selection: serde_json::Value,
    /// Traffic limit in bytes. `null` = unlimited.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traffic_limit: Option<u64>,
    /// Subscription expiry (ISO 8601). `null` = never expires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// Whether delivery is enabled.
    pub enabled: bool,
    /// The active short code string, if one has been generated. `null` = no
    /// short code. Delivery via `GET /s/{code}` resolves this to the
    /// subscription.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_code: Option<String>,
    /// Creation time (ISO 8601 UTC).
    pub created_at: String,
    /// Last update time (ISO 8601 UTC).
    pub updated_at: String,
}

/// Response body for `POST /api/v1/subscriptions`.
///
/// The `token_plaintext` is the CSPRNG-generated delivery token shown exactly
/// once at creation time. The server stores only the HMAC-SHA256 digest; the
/// plaintext is never persisted and never appears in logs (SEC-009).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubscriptionResponse {
    /// The created subscription aggregate.
    pub subscription: SubscriptionDto,
    /// The plaintext delivery token. Shown once; store it securely.
    pub token_plaintext: String,
}

/// Request body for `POST /api/v1/subscriptions/{id}/rotate-token`.
///
/// `grace_seconds` controls the rotation grace period: during grace, both the
/// old and new tokens remain valid. `null` or `-1` means permanent grace (the
/// old token stays valid indefinitely). `0` means no grace (the old token is
/// immediately invalid).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RotateTokenRequest {
    /// Grace period in seconds. `null` or `-1` = permanent grace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grace_seconds: Option<i64>,
}

/// Response body for `POST /api/v1/subscriptions/{id}/rotate-token`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TokenRotationResponse {
    /// The token row ULID (stable across rotations).
    pub token_id: String,
    /// The new plaintext delivery token. Shown once; store it securely.
    pub token_plaintext: String,
}

/// Response body for `POST /api/v1/subscriptions/{id}/regenerate-short-code`.
///
/// The short code is a CSPRNG-generated base62 string (8 chars). Unlike the
/// delivery token, it is not a secret — it is a public lookup key for
/// `GET /s/{code}`. If a short code already exists, it is replaced.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ShortCodeResponse {
    /// The short code row ULID.
    pub short_code_id: String,
    /// The public base62 short code string (e.g. `"aB3xK9mQ"`).
    pub code: String,
}

/// Request body for `POST /api/v1/subscriptions/{id}/temp-links`.
///
/// A temp link is an alternative delivery token with a mandatory expiry and
/// revocation. The plaintext is returned once at creation; only the
/// HMAC-SHA256 digest is persisted.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateTempLinkRequest {
    /// When the temp link expires (ISO 8601 UTC). Required.
    pub expires_at: String,
}

/// Response body for `POST /api/v1/subscriptions/{id}/temp-links`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateTempLinkResponse {
    /// The temp link row ULID.
    pub temp_link_id: String,
    /// The plaintext temp link token. Shown once; store it securely.
    pub token_plaintext: String,
    /// The expiry timestamp (ISO 8601 UTC).
    pub expires_at: String,
}

/// Response body for `GET /api/v1/subscriptions` (cursor-paginated).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListSubscriptionsResponse {
    /// Subscriptions in the current page.
    pub subscriptions: Vec<SubscriptionDto>,
    /// Cursor for the next page (`None` if no more results).
    pub next_cursor: Option<String>,
}

/// Response body for `GET /api/v1/subscriptions/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GetSubscriptionResponse {
    /// The subscription aggregate.
    pub subscription: SubscriptionDto,
}

/// Query parameters for `GET /api/v1/subscriptions`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListSubscriptionsQuery {
    /// Pagination cursor — the ULID of the last subscription from the
    /// previous page.
    pub cursor: Option<String>,
    /// Maximum number of subscriptions to return (default 50, max 100).
    #[serde(default)]
    pub limit: Option<u32>,
}
