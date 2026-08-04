//! Auth application module: commands for authentication and user management.
//!
//! This module orchestrates domain services and port interfaces. It does not
//! execute SQL directly. See `docs/plan/03-architecture.md` §"Lightweight
//! CQRS" and `docs/plan/milestones/M2-auth-and-users.md` for the milestone
//! blueprint.

pub mod commands;
pub mod error;
pub mod rate_limiter;

pub use commands::{
    AuthPrincipal, LoginParams, authenticate_session, create_user, disable_user, find_user,
    force_logout, list_users, login, logout, setup_admin, user_count, verify_session,
};
pub use error::AuthError;
pub use rate_limiter::LoginRateLimiter;
