//! Foundational primitives for the Deve Sub workspace: strong-typed IDs,
//! time, pagination, and shared error types.
//!
//! This crate depends on no other workspace crate and contains no domain
//! logic. See `docs/plan/04-workspace-layout.md` for the crate's scope.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod error;
pub mod id;
pub mod pagination;
pub mod revision;
pub mod time;

pub use error::{KernelError, Result};
pub use id::{NodeId, TagId};
pub use pagination::{Cursor, Page, Pagination};
pub use revision::Revision;
pub use time::Timestamp;
