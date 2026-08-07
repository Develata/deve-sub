//! Source domain module: source aggregate, snapshot entity, and port traits.
//!
//! A source is a subscription URL that is periodically fetched and parsed
//! into the unified node pool. Each refresh creates a snapshot recording
//! the fetched content and resulting node count. See
//! `docs/plan/milestones/M4-sources-and-node-pool.md`.

pub mod entity;
pub mod error;
pub mod ports;
pub mod snapshot;
pub mod source_item;

pub use entity::{Source, SourceType};
pub use error::SourceError;
pub use ports::{
    ImportOutcome, ImportResult, NodeFilter, NodePoolEntry, NodePoolRepository, ReconcileEntry,
    ReconcileInput, ReconcileResult, SourceRepository, SourceSnapshotRepository,
};
pub use snapshot::SourceSnapshot;
pub use source_item::{ItemParseStatus, SourceItem};
