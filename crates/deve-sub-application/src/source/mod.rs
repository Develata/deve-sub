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
    CreateSourceParams, ListNodesParams, RefreshResult, UpdateSourceParams, create_source,
    delete_source, get_node, get_source, import_nodes, list_nodes, list_sources, refresh_source,
    update_source,
};
pub use error::SourceAppError;
pub use fetcher::{FetchError, FetchResult, SubscriptionFetcher};
pub use parse::{ImportParseResult, ParseContentError, parse_content, parse_for_import};
pub use scheduler::RefreshScheduler;
