//! Template domain module: V3 subscription template aggregate, version
//! snapshots, spec value objects, and port traits.
//!
//! A `SubscriptionTemplate` is a versioned, declarative document conforming to
//! `apiVersion: deve-sub.io/v1` / `kind: SubscriptionTemplate`. Each edit
//! creates a new `TemplateVersion`; the active version is the one served by
//! generation. See `docs/plan/milestones/M5-generator-and-v3-template.md`.

pub mod cache;
pub mod chain;
pub mod entity;
pub mod error;
pub mod generation;
pub mod ports;
pub mod selection;
pub mod spec;

pub use cache::{CacheKeyParams, GenerationCacheEntry, GenerationCacheRepository};
pub use chain::{ChainEdge, ChainGraph, ChainVertex, CyclePath};
pub use entity::{SubscriptionTemplate, TemplateVersion};
pub use error::TemplateError;
pub use generation::{
    CompatibilityReport, ExcludedNode, GenerationError, GenerationMode, GenerationRequest,
    GenerationResult, IncompatibleGroup,
};
pub use ports::{TemplateRepository, TemplateVersionRepository};
pub use selection::{GroupResolution, MissingNodeRef, MissingReason, TemplateResolution};
pub use spec::{
    API_VERSION, FilterField, GroupMember, GroupType, KIND, MAX_ALIAS_DEPTH, MAX_SPEC_BYTES,
    NodeFilterRule, NodeSelector, ProxyGroup, QuickGroupFilter, Rule, SelectionMode, SortOrder,
    TemplateDocument, TemplateMetadata, TemplateSpec,
};
