//! Foundational primitives for the Deve Sub workspace: strong-typed IDs,
//! time, revisions, and shared error types.
//!
//! This crate depends on no other workspace crate and contains no domain
//! logic. Entity IDs are shared value objects (ULID newtypes), not domain
//! logic. See `docs/plan/04-workspace-layout.md` for the crate's scope.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod error;
pub mod id;
pub mod revision;
pub mod time;

pub use error::{KernelError, Result};
pub use id::{
    AuditLogId, GenerationCacheId, NodeId, NodeOverrideId, NodeSourceBindingId, OutboxEventId,
    RecoveryCodeId, SessionId, ShortCodeId, SourceId, SourceItemId, SourceSnapshotId,
    SubscriptionId, SubscriptionTokenId, TagId, TempLinkId, TemplateId, TemplateVersionId,
    TrafficRecordId, UserId,
};
pub use revision::Revision;
pub use time::Timestamp;
