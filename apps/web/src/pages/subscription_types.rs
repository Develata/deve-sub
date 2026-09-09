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
    TokenDisplay(String),
    TempLink(SubscriptionDto),
}
