//! Auth application commands: setup-admin, login, logout.
//!
//! These functions orchestrate domain services and port interfaces. They do
//! not execute SQL directly. One API operation maps to one command. See
//! `docs/plan/03-architecture.md` §"Lightweight CQRS".

use deve_sub_domain::{IdentityError, Role, Session, SessionRepository, User, UserRepository};
use deve_sub_kernel::{SessionId, Timestamp, UserId};
use deve_sub_security::{
    MasterKey, generate_session_token, hash_password, hash_session_token, verify_password,
};

use super::error::AuthError;

/// Minimum password length. Accounts created with shorter passwords are
/// rejected at the application boundary.
const MIN_PASSWORD_LEN: usize = 8;

/// Maximum password length. Defends against argon2 memory-exhaustion via
/// oversized inputs.
const MAX_PASSWORD_LEN: usize = 1024;

/// Maximum username length.
const MAX_USERNAME_LEN: usize = 64;

/// Validate username and password at the application boundary.
///
/// Returns [`AuthError::InvalidInput`] with a static message describing the
/// first violation. Does not allocate.
fn validate_credentials(username: &str, password: &str) -> Result<(), AuthError> {
    if username.is_empty() {
        return Err(AuthError::InvalidInput("username must not be empty"));
    }
    if username.len() > MAX_USERNAME_LEN {
        return Err(AuthError::InvalidInput(
            "username must not exceed 64 characters",
        ));
    }
    if password.len() < MIN_PASSWORD_LEN {
        return Err(AuthError::InvalidInput(
            "password must be at least 8 characters",
        ));
    }
    if password.len() > MAX_PASSWORD_LEN {
        return Err(AuthError::InvalidInput(
            "password must not exceed 1024 characters",
        ));
    }
    Ok(())
}

/// Create the first admin user.
///
/// Returns [`AuthError::AlreadyInitialized`] if any users already exist.
/// This is the initialization path for AUTH-001.
///
/// # Errors
/// - [`AuthError::AlreadyInitialized`] — users table is not empty.
/// - [`AuthError::Security`] — password hashing failed.
/// - [`AuthError::Identity`] — storage error or username collision.
pub async fn setup_admin(
    user_repo: &dyn UserRepository,
    username: &str,
    password: &str,
) -> Result<User, AuthError> {
    validate_credentials(username, password)?;
    let password_hash = hash_password(password)?;
    let user = User::new(username, password_hash, Role::Admin);
    user_repo
        .create_if_empty(&user)
        .await
        .map_err(|e| match e {
            IdentityError::AlreadyInitialized => AuthError::AlreadyInitialized,
            other => AuthError::Identity(other),
        })?;
    Ok(user)
}

/// Authenticate a user and create a session.
///
/// Returns the session and the raw token string. The raw token is given to
/// the client as a cookie; only the HMAC digest is persisted.
///
/// Returns [`AuthError::InvalidCredentials`] for unknown username, wrong
/// password, or disabled account — the same error for all three to avoid
/// leaking user existence (AUTH-003).
///
/// # Errors
/// - [`AuthError::InvalidCredentials`] — bad credentials or disabled account.
/// - [`AuthError::Security`] — token generation or hashing failed.
/// - [`AuthError::Identity`] — storage error.
// A dummy argon2id PHC hash used to equalize login timing when the
// username does not exist. Without this, the `None` branch returns
// immediately while the `Some` branch spends ~20-50ms in `verify_password`,
// leaking username existence via timing (AUTH-003).
const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

pub async fn login(
    user_repo: &dyn UserRepository,
    session_repo: &dyn SessionRepository,
    master_key: &MasterKey,
    username: &str,
    password: &str,
    session_ttl: time::Duration,
) -> Result<(User, Session, String), AuthError> {
    let user = user_repo.find_by_username(username).await?;

    // Timing side-channel mitigation: always run verify_password, even when
    // the user does not exist, so both branches take similar time.
    let user = match user {
        Some(u) => {
            if !u.is_active() {
                // WHY: still verify against the real hash to keep timing
                // uniform across disabled vs wrong-password vs unknown-user.
                let _ = verify_password(password, &u.password_hash);
                return Err(AuthError::InvalidCredentials);
            }
            if !verify_password(password, &u.password_hash)? {
                return Err(AuthError::InvalidCredentials);
            }
            u
        }
        None => {
            let _ = verify_password(password, DUMMY_PASSWORD_HASH);
            return Err(AuthError::InvalidCredentials);
        }
    };

    let token = generate_session_token()?;
    let token_hash = hash_session_token(&token, master_key.as_bytes())?;
    let expires_at = Timestamp::now() + session_ttl;
    let session = Session::new(user.id, token_hash, expires_at);
    session_repo.create(&session).await?;

    Ok((user, session, token))
}

/// Revoke a session by ID.
///
/// # Errors
/// - [`AuthError::Identity`] — storage error.
pub async fn logout(
    session_repo: &dyn SessionRepository,
    session_id: SessionId,
) -> Result<(), AuthError> {
    session_repo.revoke(session_id).await?;
    Ok(())
}

/// Disable a user and revoke all their sessions.
///
/// # Errors
/// - [`AuthError::Identity`] — storage error or user not found.
pub async fn disable_user(
    user_repo: &dyn UserRepository,
    session_repo: &dyn SessionRepository,
    user_id: UserId,
) -> Result<(), AuthError> {
    // WHY: `set_enabled` and `revoke_all_for_user` are two separate SQL
    // statements with no shared transaction. If `revoke_all_for_user` fails
    // after `set_enabled` succeeds, the user is disabled but stale session
    // rows may remain `revoked = 0`. This is safe because
    // `authenticate_session` re-checks `user.is_active()` on every request,
    // so disabled-user sessions cannot authenticate regardless of the
    // `revoked` flag. The stale rows are a storage-level cosmetic issue, not
    // a security gap.
    user_repo.set_enabled(user_id, false).await?;
    session_repo.revoke_all_for_user(user_id).await?;
    Ok(())
}

/// Verify a session token and return the associated session.
///
/// Returns `Ok(Some(session))` if the session is valid (not revoked, not
/// expired), `Ok(None)` if the token hash does not match any session or the
/// session is invalid.
///
/// # Errors
/// - [`AuthError::Security`] — token hashing failed.
/// - [`AuthError::Identity`] — storage error.
pub async fn verify_session(
    session_repo: &dyn SessionRepository,
    master_key: &MasterKey,
    token: &str,
) -> Result<Option<Session>, AuthError> {
    let token_hash = hash_session_token(token, master_key.as_bytes())?;
    let session = session_repo.find_by_token_hash(&token_hash).await?;
    Ok(session.filter(Session::is_valid))
}

/// The authenticated principal returned by [`authenticate_session`].
pub struct AuthPrincipal {
    /// The authenticated user.
    pub user: User,
    /// The active session.
    pub session: Session,
}

/// Authenticate a request by verifying the session token and loading the
/// associated user.
///
/// This is the single application query for request authentication. It
/// encapsulates session verification, user lookup, and the "active user"
/// policy check, keeping business rules in the Application layer rather than
/// the Delivery layer. Returns `Ok(None)` if the session is invalid, expired,
/// or the user is disabled/expired.
///
/// # Errors
/// - [`AuthError::Security`] — token hashing failed.
/// - [`AuthError::Identity`] — storage error.
pub async fn authenticate_session(
    session_repo: &dyn SessionRepository,
    user_repo: &dyn UserRepository,
    master_key: &MasterKey,
    token: &str,
) -> Result<Option<AuthPrincipal>, AuthError> {
    let session = verify_session(session_repo, master_key, token).await?;
    let Some(session) = session else {
        return Ok(None);
    };
    let user = user_repo
        .find_by_id(session.user_id)
        .await?
        .ok_or(AuthError::InvalidCredentials)?;
    if !user.is_active() {
        return Ok(None);
    }
    Ok(Some(AuthPrincipal { user, session }))
}

/// Find a user by ID.
///
/// # Errors
/// - [`AuthError::Identity`] — storage error.
pub async fn find_user(
    user_repo: &dyn UserRepository,
    user_id: UserId,
) -> Result<Option<User>, AuthError> {
    user_repo.find_by_id(user_id).await.map_err(Into::into)
}

/// Check if any users exist (for setup-admin gate).
///
/// # Errors
/// - [`AuthError::Identity`] — storage error.
pub async fn user_count(user_repo: &dyn UserRepository) -> Result<i64, AuthError> {
    user_repo.count().await.map_err(Into::into)
}

/// Create a new user (admin-only operation).
///
/// Hashes the password and stores the user. Returns
/// [`AuthError::Identity`] with [`IdentityError::UsernameExists`] if the
/// username is already taken.
///
/// # Errors
/// - [`AuthError::Security`] — password hashing failed.
/// - [`AuthError::Identity`] — storage error or username collision.
pub async fn create_user(
    user_repo: &dyn UserRepository,
    username: &str,
    password: &str,
    role: Role,
) -> Result<User, AuthError> {
    validate_credentials(username, password)?;
    let password_hash = hash_password(password)?;
    let user = User::new(username, password_hash, role);
    user_repo.create(&user).await?;
    Ok(user)
}

/// List users with cursor pagination.
///
/// Returns up to `limit` users whose ULID is greater than `cursor` (or all
/// if `cursor` is `None`). The caller is responsible for deriving the next
/// cursor from the last element's ID.
///
/// # Errors
/// - [`AuthError::Identity`] — storage error.
pub async fn list_users(
    user_repo: &dyn UserRepository,
    cursor: Option<UserId>,
    limit: u32,
) -> Result<Vec<User>, AuthError> {
    user_repo.list(cursor, limit).await.map_err(Into::into)
}

/// Force logout: revoke all sessions for a user without disabling the
/// account. The user may log in again immediately (AUTH-010).
///
/// # Errors
/// - [`AuthError::Identity`] — storage error.
pub async fn force_logout(
    session_repo: &dyn SessionRepository,
    user_id: UserId,
) -> Result<(), AuthError> {
    session_repo.revoke_all_for_user(user_id).await?;
    Ok(())
}
