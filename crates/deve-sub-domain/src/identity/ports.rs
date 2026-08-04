//! Port traits for identity storage.
//!
//! These traits define the storage boundary for users and sessions. The
//! SQLite adapter implements them; the application layer calls them. See
//! ADR-0002 for the storage Port decision.

use async_trait::async_trait;

use deve_sub_kernel::{SessionId, UserId};

use super::error::IdentityError;
use super::{Session, User};

/// Storage boundary for user aggregates.
#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Create a new user. Returns [`IdentityError::UsernameExists`] if the
    /// username is already taken.
    async fn create(&self, user: &User) -> Result<(), IdentityError>;

    /// Find a user by ID.
    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, IdentityError>;

    /// Find a user by username.
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, IdentityError>;

    /// Count all users.
    async fn count(&self) -> Result<i64, IdentityError>;

    /// Create a user only if no users exist yet.
    ///
    /// This is the atomic "first admin" gate: the check and insert happen
    /// in a single SQL statement to prevent a TOCTOU race between two
    /// concurrent `setup_admin` calls. Returns
    /// [`IdentityError::AlreadyInitialized`] if any users already exist.
    async fn create_if_empty(&self, user: &User) -> Result<(), IdentityError>;

    /// Set the enabled flag for a user. Disabling a user should also revoke
    /// all their sessions (enforced by the application layer).
    async fn set_enabled(&self, id: UserId, enabled: bool) -> Result<(), IdentityError>;
}

/// Storage boundary for session entities.
#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// Create a new session.
    async fn create(&self, session: &Session) -> Result<(), IdentityError>;

    /// Find a session by its token hash.
    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<Session>, IdentityError>;

    /// Mark a session as revoked.
    async fn revoke(&self, id: SessionId) -> Result<(), IdentityError>;

    /// Revoke all sessions for a user.
    async fn revoke_all_for_user(&self, user_id: UserId) -> Result<(), IdentityError>;
}
