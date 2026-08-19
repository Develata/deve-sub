//! DTO types for the subscriptions page, matching `deve-sub-contract::subscription`.

#![cfg(target_family = "wasm")]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionDto {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub owner_id: String,
    pub template_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_version_pin: Option<u64>,
    pub profile: String,
    pub node_selection: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traffic_limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSubscriptionsResponse {
    pub subscriptions: Vec<SubscriptionDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateSubscriptionRequest {
    pub name: String,
    pub slug: String,
    pub template_id: String,
    pub profile: String,
    pub node_selection: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traffic_limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateSubscriptionRequest {
    pub name: String,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_version_pin: Option<u64>,
    pub profile: String,
    pub node_selection: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traffic_limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionResponse {
    pub subscription: SubscriptionDto,
    pub token_plaintext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSubscriptionResponse {
    pub subscription: SubscriptionDto,
}

#[derive(Debug, Clone, Serialize)]
pub struct RotateTokenRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grace_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRotationResponse {
    pub token_id: String,
    pub token_plaintext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortCodeResponse {
    pub short_code_id: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateTempLinkRequest {
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTempLinkResponse {
    pub temp_link_id: String,
    pub token_plaintext: String,
    pub expires_at: String,
}

pub const PROFILES: &[&str] = &[
    "mihomo",
    "sing-box",
    "xray",
    "v2ray",
    "shadowrocket",
    "uri_list",
];

/// Modal state machine for the subscriptions page.
#[derive(Clone, PartialEq)]
pub enum Modal {
    None,
    Create,
    Edit(SubscriptionDto),
    Delete(SubscriptionDto),
    TokenDisplay(String),
    TempLink(SubscriptionDto),
}
