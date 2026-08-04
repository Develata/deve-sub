//! Port traits for identity storage.
//!
//! These traits define the storage boundary for users, sessions, TOTP secrets,
//! and recovery codes. The SQLite adapter implements them; the application
//! layer calls them. See ADR-0002 for the storage Port decision.

use async_trait::async_trait;

use deve_sub_kernel::{RecoveryCodeId, SessionId, UserId};

use super::error::IdentityError;
use super::{RecoveryCode, Session, TotpSecret, User};

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

    /// List users with cursor pagination.
    ///
    /// Returns up to `limit` users whose ULID is strictly greater than
    /// `cursor` (or all users if `cursor` is `None`), ordered by `id`.
    /// ULIDs are lexically sortable by creation time, so the cursor is the
    /// last user's ID from the previous page.
    async fn list(&self, cursor: Option<UserId>, limit: u32) -> Result<Vec<User>, IdentityError>;

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

    /// Set the 2FA-enabled flag for a user.
    async fn set_two_factor_enabled(&self, id: UserId, enabled: bool) -> Result<(), IdentityError>;

    /// Update the last login timestamp for a user.
    async fn update_last_login(
        &self,
        id: UserId,
        at: deve_sub_kernel::Timestamp,
    ) -> Result<(), IdentityError>;
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

/// Storage boundary for encrypted TOTP secrets.
#[async_trait]
pub trait TotpSecretRepository: Send + Sync {
    /// Upsert the TOTP secret for a user. Replaces any existing secret.
    async fn upsert(&self, secret: &TotpSecret) -> Result<(), IdentityError>;

    /// Find the TOTP secret for a user.
    async fn find_by_user(&self, user_id: UserId) -> Result<Option<TotpSecret>, IdentityError>;

    /// Delete the TOTP secret for a user.
    async fn delete(&self, user_id: UserId) -> Result<(), IdentityError>;
}

/// Storage boundary for 2FA recovery codes.
#[async_trait]
pub trait RecoveryCodeRepository: Send + Sync {
    /// Atomically delete all existing recovery codes for a user and store a
    /// new batch in a single transaction.
    ///
    /// This ensures there is never a window where the user has zero recovery
    /// codes (e.g. during regeneration or initial 2FA setup).
    async fn replace_all_for_user(
        &self,
        user_id: UserId,
        codes: &[RecoveryCode],
    ) -> Result<(), IdentityError>;

    /// Find an unused recovery code by its hash for a specific user.
    async fn find_unused_by_hash(
        &self,
        user_id: UserId,
        code_hash: &str,
    ) -> Result<Option<RecoveryCode>, IdentityError>;

    /// Mark a recovery code as used.
    async fn mark_used(&self, id: RecoveryCodeId) -> Result<(), IdentityError>;

    /// Delete all recovery codes for a user.
    async fn delete_all_for_user(&self, user_id: UserId) -> Result<(), IdentityError>;
}
