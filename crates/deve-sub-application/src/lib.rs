//! Application layer for Deve Sub: commands, queries, and use cases.
//!
//! This crate orchestrates domain services and port interfaces. It does not
//! execute SQL directly or contain framework types. See
//! `docs/plan/03-architecture.md` for the application layer's position in the
//! hexagonal architecture and the lightweight CQRS pattern.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod auth;
pub mod config;
pub mod health;
pub mod source;

pub use auth::{AuthError, LoginRateLimiter};
pub use config::AppConfig;
pub use health::{DbHealthPort, HealthError, HealthStatus, HealthView};
pub use source::SourceAppError;
