//! API DTOs, event DTOs, and client capability contracts shared across the
//! delivery and application layers.
//!
//! `ToSchema` implementations for OpenAPI generation live here. See
//! `docs/plan/03-architecture.md` and ADR-0004 for the API boundary policy.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod health;

pub use health::{HealthLiveResponse, HealthReadyResponse, HealthStatusDto};
