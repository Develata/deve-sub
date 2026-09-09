//! Shared API wire types and local presentation state.

#![cfg(target_family = "wasm")]

pub use deve_sub_contract::{AuditLogDto, ListAuditLogsResponse};

// WHY: these constants must match the action/target_type strings the
// server actually writes (see `deve-sub-application/src/audit/commands.rs`).
// A drift here makes the audit filter silently return zero rows for the
// mismatched actions and hide real audited events from the UI (SV-001).
pub const ACTIONS: &[&str] = &[
    "auth.login",
    "auth.logout",
    "auth.2fa.enable",
    "auth.2fa.disable",
    "user.create",
    "user.disable",
    "user.force_logout",
    "source.create",
    "source.update",
    "source.delete",
    "source.refresh",
    "subscription.create",
    "subscription.update",
    "subscription.delete",
    "subscription.token.rotate",
    "template.create",
    "template.update",
    "template.delete",
    "template.rollback",
    "probe.source.create",
    "probe.source.update",
    "probe.source.delete",
    "probe.source.sync",
    "probe.run.start",
    "probe.run.cancel",
];

pub const TARGET_TYPES: &[&str] = &[
    "user",
    "source",
    "subscription",
    "template",
    "probe_source",
    "probe_run",
];
