//! Template domain module: V3 subscription template aggregate, version
//! snapshots, spec value objects, and port traits.
//!
//! A `SubscriptionTemplate` is a versioned, declarative document conforming to
//! `apiVersion: deve-sub.io/v1` / `kind: SubscriptionTemplate`. Each edit
//! creates a new `TemplateVersion`; the active version is the one served by
//! generation. See `docs/plan/milestones/M5-generator-and-v3-template.md`.

pub mod entity;
pub mod error;
pub mod ports;
pub mod spec;

pub use entity::{SubscriptionTemplate, TemplateVersion};
pub use error::TemplateError;
pub use ports::{TemplateRepository, TemplateVersionRepository};
pub use spec::{
    API_VERSION, FilterField, GroupMember, GroupType, KIND, MAX_ALIAS_DEPTH, MAX_SPEC_BYTES,
    NodeFilterRule, NodeSelector, ProxyGroup, QuickGroupFilter, Rule, SelectionMode, SortOrder,
    TemplateDocument, TemplateMetadata, TemplateSpec,
};
