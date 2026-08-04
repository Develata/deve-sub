//! Auth application module: commands for authentication, user management,
//! and 2FA.
//!
//! This module orchestrates domain services and port interfaces. It does
//! not execute SQL directly. See `docs/plan/03-architecture.md` §"Lightweight
//! CQRS" and `docs/plan/milestones/M2-auth-and-users.md` for the milestone
//! blueprint.

pub mod challenge;
pub mod commands;
pub mod error;
pub mod rate_limiter;
pub mod twofa;

pub use challenge::{generate_challenge_token, verify_challenge_token};
pub use commands::{
    AuthPrincipal, LoginOutcome, LoginParams, authenticate_session, create_user, disable_user,
    find_user, force_logout, list_users, login, logout, setup_admin, user_count, verify_session,
};
pub use error::AuthError;
pub use rate_limiter::LoginRateLimiter;
pub use twofa::{
    LoginTwoFactorParams, TwoFactorSetupResult, TwoFactorVerifyResult, disable_2fa, login_2fa,
    regenerate_recovery_codes, setup_2fa, verify_2fa,
};
