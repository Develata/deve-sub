//! Source DTOs for the `/api/v1/sources` endpoints.
//!
//! These DTOs are the wire format for subscription source management. They are
//! owned by the contract crate per ADR-0004: DTOs and `ToSchema` derives live
//! here, not in the API crate.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Input format of a subscription source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceTypeDto {
    /// Auto-detect from content type and body.
    Auto,
    /// Base64-encoded URI list.
    Base64,
    /// One URI per line.
    UriList,
    /// Mihomo (Clash) YAML.
    MihomoYaml,
    /// sing-box JSON.
    SingboxJson,
    /// Xray JSON.
    XrayJson,
    /// V2Ray JSON.
    V2rayJson,
    /// Shadowrocket share list.
    Shadowrocket,
}

/// Source information returned by source management endpoints.
///
/// Never includes encrypted headers ciphertext in responses.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SourceDto {
    /// ULID identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Input format.
    pub source_type: SourceTypeDto,
    /// Subscription URL.
    pub url: String,
    /// Whether automatic refresh is enabled.
    pub auto_update: bool,
    /// Refresh interval in seconds.
    pub update_interval_secs: u64,
    /// Whether the source is active.
    pub enabled: bool,
    /// Whether to keep existing nodes if a refresh fails.
    pub keep_on_fail: bool,
    /// Account creation time (ISO 8601 UTC).
    pub created_at: String,
}

/// Request body for `POST /api/v1/sources`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateSourceRequest {
    /// Human-readable name.
    pub name: String,
    /// Input format. `auto` lets the fetcher detect.
    pub source_type: SourceTypeDto,
    /// Subscription URL.
    pub url: String,
    /// Whether automatic refresh is enabled.
    #[serde(default)]
    pub auto_update: bool,
    /// Refresh interval in seconds.
    #[serde(default = "default_update_interval")]
    pub update_interval_secs: u64,
    /// Whether to keep existing nodes if a refresh fails.
    #[serde(default = "default_keep_on_fail")]
    pub keep_on_fail: bool,
}

/// Request body for `PUT /api/v1/sources/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateSourceRequest {
    /// Human-readable name.
    pub name: String,
    /// Input format.
    pub source_type: SourceTypeDto,
    /// Subscription URL.
    pub url: String,
    /// Whether automatic refresh is enabled.
    pub auto_update: bool,
    /// Refresh interval in seconds.
    pub update_interval_secs: u64,
    /// Whether the source is active.
    pub enabled: bool,
    /// Whether to keep existing nodes if a refresh fails.
    pub keep_on_fail: bool,
}

/// Response body for `POST /api/v1/sources` and `PUT /api/v1/sources/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SourceResponse {
    /// The source.
    pub source: SourceDto,
}

/// Response body for `GET /api/v1/sources` (cursor-paginated source list).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListSourcesResponse {
    /// Sources in the current page.
    pub sources: Vec<SourceDto>,
    /// Cursor for the next page (`None` if no more results).
    pub next_cursor: Option<String>,
}

fn default_update_interval() -> u64 {
    3600
}

fn default_keep_on_fail() -> bool {
    true
}
