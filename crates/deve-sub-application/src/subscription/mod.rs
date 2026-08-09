//! Subscription application module: commands for subscription lifecycle and
//! delivery token management.
//!
//! This module orchestrates domain services and port interfaces. It does not
//! execute SQL directly. See `docs/plan/03-architecture.md` §"Lightweight
//! CQRS" and `docs/plan/milestones/M6-subscription-distribution.md` for the
//! milestone blueprint.

pub mod commands;
pub mod delivery;
pub mod error;

pub use commands::{
    CreateSubscriptionParams, CreateSubscriptionResult, CreateTempLinkParams, CreateTempLinkResult,
    RotateTokenResult, ShortCodeResult, UpdateSubscriptionParams, create_subscription,
    create_temp_link, delete_subscription, get_subscription, list_subscriptions,
    regenerate_short_code, revoke_temp_link, rotate_token, update_subscription,
};
pub use delivery::{
    DeliveryDeps, DeliveryResult, deliver_by_short_code, deliver_by_temp_link,
    deliver_subscription, detect_profile_from_user_agent,
};
pub use error::SubscriptionAppError;
