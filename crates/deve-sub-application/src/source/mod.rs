//! Source application module: commands and queries for subscription sources.
//!
//! This module orchestrates domain services and port interfaces. It does not
//! execute SQL directly. See `docs/plan/03-architecture.md` §"Lightweight
//! CQRS" and `docs/plan/milestones/M4-sources-and-node-pool.md` for the
//! milestone blueprint.

pub mod commands;
pub mod error;

pub use commands::{
    CreateSourceParams, UpdateSourceParams, create_source, delete_source, get_source, list_sources,
    update_source,
};
pub use error::SourceAppError;
