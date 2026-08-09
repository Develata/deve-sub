//! Subscription domain module: subscription aggregate, delivery token, short
//! code, temp link, and port traits.
//!
//! A `Subscription` is an independent aggregate root binding a template with
//! its own node selection and delivery configuration. A `SubscriptionToken`
//! carries the HMAC-SHA256 digest of the CSPRNG-generated plaintext token. A
//! `ShortCode` is a public base62 lookup key for `GET /s/{code}` delivery. A
//! `TempLink` is a revocable, expiry-bounded alternative delivery token. See
//! `docs/plan/milestones/M6-subscription-distribution.md`.

pub mod entity;
pub mod error;
pub mod ports;

pub use entity::{ShortCode, Subscription, SubscriptionToken, TempLink};
pub use error::SubscriptionError;
pub use ports::{
    ShortCodeRepository, SubscriptionRepository, SubscriptionTokenRepository, TempLinkRepository,
};
