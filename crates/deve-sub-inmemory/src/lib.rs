//! In-memory adapter implementations for Deve Sub application ports.
//!
//! This crate houses infrastructure adapters that implement application-layer
//! port traits without backing by an external system. See
//! `docs/plan/04-workspace-layout.md` §"Dependency direction" — adapter crates
//! depend on Port traits defined in `crates/application`, not the other way
//! around.

pub mod rate_limiter;

pub use rate_limiter::InMemoryLoginRateLimiter;
