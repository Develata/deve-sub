//! Subscription domain module: subscription aggregate, delivery token, and
//! port traits.
//!
//! A `Subscription` is an independent aggregate root binding a template with
//! its own node selection and delivery configuration. A `SubscriptionToken`
//! carries the HMAC-SHA256 digest of the CSPRNG-generated plaintext token.
//! See `docs/plan/milestones/M6-subscription-distribution.md`.

pub mod entity;
pub mod error;
pub mod ports;

pub use entity::{Subscription, SubscriptionToken};
pub use error::SubscriptionError;
pub use ports::{SubscriptionRepository, SubscriptionTokenRepository};
