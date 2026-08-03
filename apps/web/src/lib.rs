//! Deve Sub web shell placeholder.
//!
//! In M1 this serves a minimal static HTML placeholder. The full Dioxus Web
//! frontend arrives in a later milestone. See ADR-0001 for the frontend mode
//! decision.

#![cfg_attr(test, allow(clippy::expect_used))]

/// Static HTML placeholder for the web shell.
///
/// The server serves this at `/` when `serve_web` is enabled.
pub const PLACEHOLDER_HTML: &str = include_str!("placeholder.html");
