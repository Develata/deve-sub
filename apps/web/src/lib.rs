//! Deve Sub web shell.
//!
//! On native targets, this crate exports `PLACEHOLDER_HTML` for the server
//! to serve as a fallback. The full Dioxus Web frontend is compiled
//! separately via `dx build` for the `wasm32-unknown-unknown` target and
//! served from `apps/web/dist/`.
//!
//! See ADR-0001 for the frontend mode decision.

#![cfg_attr(test, allow(clippy::expect_used))]

/// Static HTML placeholder served by the server when the compiled
/// frontend is not available.
pub const PLACEHOLDER_HTML: &str = include_str!("placeholder.html");
