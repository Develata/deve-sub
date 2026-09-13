//! Audit log domain model: append-only record of actor actions on targets.
//!
//! The audit log captures who did what to which entity, with optional
//! non-sensitive metadata. Events are immutable and subject to explicit retention. See
//! `docs/plan/milestones/M10-observability-and-audit.md`.

pub mod entity;
pub mod error;
pub mod ports;

pub use entity::AuditLog;
pub use error::AuditError;
pub use ports::{AUDIT_CLEANUP_BATCH, AuditCleanupPreview, AuditLogFilter, AuditLogRepository};
