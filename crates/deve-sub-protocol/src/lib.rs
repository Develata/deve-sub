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

mod anytls;
pub mod container;
pub mod error;
mod hysteria2;
mod naive;
mod shadowsocks;
mod snell;
mod transport;
mod trojan;
mod tuic_v5;
mod uri;
mod vless_reality;
mod vmess;
mod wireguard;

pub use error::ParseError;

pub use uri::parse_uri;
