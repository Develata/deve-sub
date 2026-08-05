//! Input parsing for share URIs and container formats.
//!
//! Each parser maps an input format to a canonical
//! [`deve_sub_domain::Node`]. The top-level [`parse_uri`] dispatches on the
//! URI scheme. See `docs/plan/05-protocol-engine.md` and
//! `docs/plan/milestones/M3-protocol-engine.md`.
//!
//! # Errors
//! All parsers return [`ParseError`], a structured `thiserror` enum. No
//! `anyhow` in public APIs.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod error;
mod uri;
mod vless_reality;

pub use error::ParseError;

pub use uri::parse_uri;
