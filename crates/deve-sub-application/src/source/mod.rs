//! Source application module: commands and queries for subscription sources.
//!
//! This module orchestrates domain services and port interfaces. It does not
//! execute SQL directly. See `docs/plan/03-architecture.md` §"Lightweight
//! CQRS" and `docs/plan/milestones/M4-sources-and-node-pool.md` for the
//! milestone blueprint.

pub mod commands;
pub mod error;
pub mod fetcher;
pub mod parse;
pub mod scheduler;

pub use commands::{
    CreateSourceParams, RefreshResult, UpdateSourceParams, create_source, delete_source,
    get_source, list_sources, refresh_source, update_source,
};
pub use error::SourceAppError;
pub use fetcher::{FetchError, FetchResult, SubscriptionFetcher};
pub use parse::{ParseContentError, parse_content};
pub use scheduler::RefreshScheduler;
