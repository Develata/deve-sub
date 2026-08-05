//! Output format emission for canonical nodes.
//!
//! Each emitter maps a canonical [`deve_sub_domain::Node`] to a share URI or
//! target format string. The top-level [`emit_uri`] dispatches on
//! `node.protocol`. See `docs/plan/05-protocol-engine.md` and
//! `docs/plan/milestones/M3-protocol-engine.md`.
//!
//! # Errors
//! All emitters return [`EmitError`], a structured `thiserror` enum. No
//! `anyhow` in public APIs.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod error;
mod uri;
mod vless_reality;

pub use error::EmitError;

pub use uri::emit_uri;
