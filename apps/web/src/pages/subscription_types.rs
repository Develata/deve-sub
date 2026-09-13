//! Shared API wire types and local presentation state.

#![cfg(target_family = "wasm")]

pub use deve_sub_contract::{
    CreateSubscriptionRequest, CreateTempLinkRequest, CreateTempLinkResponse,
    GetSubscriptionResponse, ListSubscriptionsResponse, RotateTokenRequest, ShortCodeResponse,
    SubscriptionDto, SubscriptionResponse, TokenRotationResponse, UpdateSubscriptionRequest,
};

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
    Rotate(SubscriptionDto),
    TokenDisplay(String),
    TempLink(SubscriptionDto),
}

/// Render the credential returned by the API as a client-importable URL.
pub fn delivery_url(namespace: &str, credential: &str, profile: &str) -> String {
    let origin = web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default();
    format!("{origin}/{namespace}/{credential}/{profile}")
}
