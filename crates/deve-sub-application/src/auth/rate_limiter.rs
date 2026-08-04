//! Login rate limiter port.
//!
//! This trait defines the boundary for login attempt tracking and temporary
//! lockout. The in-memory adapter lives in the server crate; the application
//! layer calls these methods from the `login` command. See
//! `docs/plan/milestones/M2-auth-and-users.md` Slice 3 (AUTH-004).

use super::error::AuthError;

/// Rate limiter for login attempts.
///
/// Tracks failed login attempts per username and per IP address. After
/// `max_attempts` failures, the key is temporarily locked for
/// `lockout_duration`. Successful logins reset the counter.
///
/// The trait methods are synchronous because the initial implementation is
/// in-memory. If a database-backed implementation is needed later, the trait
/// can be changed to `async_trait`.
pub trait LoginRateLimiter: Send + Sync {
    /// Check if login is allowed for the given username and optional IP.
    ///
    /// Returns `Err(AuthError::RateLimited)` if either the username or IP
    /// is currently locked.
    fn check(&self, username: &str, ip: Option<&str>) -> Result<(), AuthError>;

    /// Record a failed login attempt. Increments the failure counter for
    /// both the username and IP. If the counter reaches `max_attempts`,
    /// the key is locked for `lockout_duration`.
    fn record_failure(&self, username: &str, ip: Option<&str>);

    /// Record a successful login. Resets the failure counters for both
    /// the username and IP.
    fn record_success(&self, username: &str, ip: Option<&str>);
}
