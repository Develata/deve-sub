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

mod anytls;
mod common;
mod container;
pub mod error;
mod hysteria2;
mod json;
mod naive;
mod shadowsocks;
mod shadowtls;
mod snell;
mod transport;
mod trojan;
mod tuic_v5;
mod uri;
mod uri_list;
mod vless_reality;
mod vmess;
mod wireguard;

pub use container::{
    AssembledGroup, AssembledTemplate, emit_mihomo, emit_mihomo_full, emit_shadowrocket,
    emit_singbox, emit_v2ray, emit_xray,
};
pub use error::EmitError;
pub use json::emit_json;

pub use uri::emit_uri;
pub use uri_list::emit_uri_list;
