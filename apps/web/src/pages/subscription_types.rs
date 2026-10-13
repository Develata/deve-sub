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
    Regenerate(SubscriptionDto),
    TokenDisplay(String),
    TempLink(SubscriptionDto),
}

/// Render the credential returned by the API as a client-importable URL.
pub fn delivery_url(namespace: &str, credential: &str, profile: &str) -> String {
    let origin = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_default();
    format!("{origin}/{namespace}/{credential}/{profile}")
}

/// A quota draft that cannot represent the existing API limit range.
#[derive(Debug)]
pub struct InvalidTrafficLimit;

/// Preserve invalid quota text as a draft; only blank means unlimited.
pub fn parse_traffic_limit(draft: &str) -> Result<Option<u64>, InvalidTrafficLimit> {
    if draft.is_empty() {
        return Ok(None);
    }
    match draft.parse::<u64>() {
        Ok(value)
            if value > 0
                && value <= i64::MAX as u64
                && draft.bytes().all(|b| b.is_ascii_digit()) =>
        {
            Ok(Some(value))
        }
        _ => Err(InvalidTrafficLimit),
    }
}
