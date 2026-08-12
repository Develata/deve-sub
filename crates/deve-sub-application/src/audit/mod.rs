//! Audit log application module: commands and queries for recording and
//! querying audit log entries.
//!
//! See `docs/plan/milestones/M10-observability-and-audit.md`.

pub mod commands;

pub use commands::{
    audit_2fa_disable, audit_2fa_enable, audit_force_logout, audit_login, audit_logout,
    audit_user_create, audit_user_disable, list_audit_logs, record_audit_log,
};
