//! Identity module: user, session, role, and port traits.
//!
//! This module owns the domain model for authentication and authorization.
//! It defines the `User` aggregate, `Session` entity, `Role` enum, and the
//! `UserRepository` / `SessionRepository` port traits. Adapters implement
//! the port traits; the application layer calls them. See
//! `docs/plan/03-architecture.md` for the hexagonal layering.

pub mod error;
pub mod ports;
pub mod session;
pub mod user;

pub use error::IdentityError;
pub use ports::{SessionRepository, UserRepository};
pub use session::Session;
pub use user::{Role, User};
