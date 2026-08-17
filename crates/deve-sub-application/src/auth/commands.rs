//! Auth application commands: setup-admin, login, logout.
//!
//! These functions orchestrate domain services and port interfaces. They do
//! not execute SQL directly. One API operation maps to one command. See
//! `docs/plan/03-architecture.md` §"Lightweight CQRS".

use deve_sub_domain::{IdentityError, Role, Session, SessionRepository, User, UserRepository};
use deve_sub_kernel::{SessionId, Timestamp, UserId};
use deve_sub_security::{
    MasterKey, PURPOSE_SESSION, generate_session_token, hash_password_async, hmac_digest,
    verify_password_async,
};
use std::sync::LazyLock;
use tokio::sync::Semaphore;

use super::challenge::generate_challenge_token;
use super::error::AuthError;
use super::rate_limiter::LoginRateLimiter;

/// Serializes setup_admin across concurrent callers. WHY: without this,
/// multiple pre-init requests all observe `count()==0` and each spends
/// ~20-50ms on Argon2 before the atomic `create_if_empty` rejects the
/// losers — a cheap CPU exhaustion vector. The permit spans count+hash+create
/// so only the first request pays the Argon2 cost; later callers see
/// `count()>0` and return immediately (DS-AUD-035).
static SETUP_PERMIT: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(1));

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

    // DS-AUD-035: the permit serializes count+hash+create so only one
    // concurrent caller reaches Argon2. Later callers see count()>0 and
    // return before hashing.
    let _permit = SETUP_PERMIT.acquire().await.map_err(|e| {
        AuthError::Security(deve_sub_security::SecurityError::Crypto(format!(
            "setup semaphore closed: {e}"
        )))
    })?;

    if user_repo.count().await? > 0 {
        return Err(AuthError::AlreadyInitialized);
    }

    let password_hash = hash_password_async(password.to_owned()).await?;
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

/// Check whether an admin user has been created yet.
///
/// Side-effect-free query used by `GET /api/v1/auth/status` to let the
/// client decide between the setup wizard and the login page without
/// probing `POST /auth/setup` with dummy credentials (DS-AUD-002).
///
/// # Errors
/// - [`AuthError::Identity`] — storage error.
pub async fn is_initialized(user_repo: &dyn UserRepository) -> Result<bool, AuthError> {
    Ok(user_repo.count().await? > 0)
}

// A dummy argon2id PHC hash used to equalize login timing when the
// username does not exist. Without this, the `None` branch returns
// immediately while the `Some` branch spends ~20-50ms in `verify_password`,
// leaking username existence via timing (AUTH-003).
const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// The outcome of a [`login`] attempt.
///
/// When 2FA is enabled, login returns [`LoginOutcome::TwoFactorRequired`]
/// instead of creating a session. The client must complete the 2FA flow
/// using the challenge token.
pub enum LoginOutcome {
    /// Login succeeded — session created.
    Success {
        /// The authenticated user.
        user: User,
        /// The created session.
        session: Session,
        /// The raw session token (for the cookie).
        token: String,
    },
    /// 2FA verification required.
    TwoFactorRequired {
        /// The authenticated user (password verified).
        user: User,
        /// Stateless challenge token for the `POST /login/2fa` endpoint.
        challenge_token: String,
    },
}

/// Parameters for the [`login`] command.
///
/// Bundling the repositories, rate limiter, and master key into a struct
/// avoids a clippy `too_many_arguments` warning and makes future additions
/// non-breaking.
pub struct LoginParams<'a> {
    pub user_repo: &'a dyn UserRepository,
    pub session_repo: &'a dyn SessionRepository,
    pub rate_limiter: &'a dyn LoginRateLimiter,
    pub master_key: &'a MasterKey,
    pub username: &'a str,
    pub password: &'a str,
    pub ip: Option<&'a str>,
    pub session_ttl: time::Duration,
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
/// - [`AuthError::RateLimited`] — too many failed attempts (AUTH-004).
/// - [`AuthError::Security`] — token generation or hashing failed.
/// - [`AuthError::Identity`] — storage error.
pub async fn login(params: LoginParams<'_>) -> Result<LoginOutcome, AuthError> {
    let LoginParams {
        user_repo,
        session_repo,
        rate_limiter,
        master_key,
        username,
        password,
        ip,
        session_ttl,
    } = params;
    // WHY: check the rate limiter BEFORE looking up the user so that
    // non-existent usernames also get rate-limited. This prevents username
    // enumeration via rate-limiting behavior differences (AUTH-003).
    rate_limiter.check(username, ip)?;

    let user = user_repo.find_by_username(username).await?;

    // Timing side-channel mitigation: always run verify_password, even when
    // the user does not exist, so both branches take similar time.
    let user = match user {
        Some(u) => {
            if !u.is_active() {
                // WHY: still verify against the real hash to keep timing
                // uniform across disabled vs wrong-password vs unknown-user.
                let _ = verify_password_async(password.to_owned(), u.password_hash.clone()).await;
                rate_limiter.record_failure(username, ip);
                return Err(AuthError::InvalidCredentials);
            }
            if !verify_password_async(password.to_owned(), u.password_hash.clone()).await? {
                rate_limiter.record_failure(username, ip);
                return Err(AuthError::InvalidCredentials);
            }
            u
        }
        None => {
            let _ =
                verify_password_async(password.to_owned(), DUMMY_PASSWORD_HASH.to_owned()).await;
            rate_limiter.record_failure(username, ip);
            return Err(AuthError::InvalidCredentials);
        }
    };

    // WHY: if 2FA is enabled, do NOT create a session or record rate-limiter
    // success. The login is not complete until the TOTP code is verified.
    // TOTP failures accumulate in the same rate-limiter counter as password
    // failures, preventing unlimited TOTP brute-force attempts.
    if user.two_factor_enabled {
        let challenge_token = generate_challenge_token(user.id, master_key)?;
        return Ok(LoginOutcome::TwoFactorRequired {
            user,
            challenge_token,
        });
    }

    let now = Timestamp::now();
    let token = generate_session_token()?;
    let token_hash = hmac_digest(PURPOSE_SESSION, &token, master_key.as_bytes())?;
    let expires_at = now + session_ttl;
    let session = Session::new(user.id, token_hash, expires_at);
    session_repo.create(&session).await?;

    // WHY: update last_login_at on every successful non-2FA login, matching
    // the login_2fa path. `let _ =` ignores storage failure — the login
    // already succeeded; last_login_at is a cosmetic field, not a security
    // invariant.
    let _ = user_repo.update_last_login(user.id, now).await;

    rate_limiter.record_success(username, ip);

    Ok(LoginOutcome::Success {
        user,
        session,
        token,
    })
}

/// Revoke a session by ID.
///
/// Idempotent: if the session was already revoked (or does not exist),
/// returns `Ok(())`. This handles concurrent logout races (e.g. dual-tab
/// logout) where the second `revoke` call finds `rows_affected = 0`.
///
/// # Errors
/// - [`AuthError::Identity`] — storage error.
pub async fn logout(
    session_repo: &dyn SessionRepository,
    session_id: SessionId,
) -> Result<(), AuthError> {
    match session_repo.revoke(session_id).await {
        Ok(()) => Ok(()),
        // WHY: SessionNotFound means the session was already revoked or
        // never existed. Treat as success for idempotent logout.
        Err(deve_sub_domain::IdentityError::SessionNotFound) => Ok(()),
        Err(e) => Err(AuthError::Identity(e)),
    }
}

/// Disable a user and revoke all their sessions.
///
/// `requester_id` is the admin requesting the action. Self-disable is
/// rejected with [`AuthError::SelfDisableForbidden`] to prevent an
/// unrecoverable admin lockout.
///
/// # Errors
/// - [`AuthError::SelfDisableForbidden`] — `target_id == requester_id`.
/// - [`AuthError::Identity`] — storage error or user not found.
pub async fn disable_user(
    user_repo: &dyn UserRepository,
    session_repo: &dyn SessionRepository,
    requester_id: UserId,
    target_id: UserId,
) -> Result<(), AuthError> {
    // WHY: an admin disabling their own account would lock them out with no
    // API recovery path (there is no enable-user endpoint, and setup_admin
    // refuses once users exist). Reject self-disable to prevent this. This
    // check lives in the application layer, not the delivery layer, so that
    // the business rule is enforced regardless of the entry point.
    if target_id == requester_id {
        return Err(AuthError::SelfDisableForbidden);
    }

    // WHY: `set_enabled` and `revoke_all_for_user` are two separate SQL
    // statements with no shared transaction. If `revoke_all_for_user` fails
    // after `set_enabled` succeeds, the user is disabled but stale session
    // rows may remain `revoked = 0`. This is safe because
    // `authenticate_session` re-checks `user.is_active()` on every request,
    // so disabled-user sessions cannot authenticate regardless of the
    // `revoked` flag. The stale rows are a storage-level cosmetic issue, not
    // a security gap.
    user_repo.set_enabled(target_id, false).await?;
    session_repo.revoke_all_for_user(target_id).await?;
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
    let token_hash = hmac_digest(PURPOSE_SESSION, token, master_key.as_bytes())?;
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
    let password_hash = hash_password_async(password.to_owned()).await?;
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
/// Verifies the user exists before revoking so that a mistyped ULID surfaces
/// as [`IdentityError::UserNotFound`] rather than a silent no-op
/// (`revoke_all_for_user` does not report whether any sessions existed).
///
/// # Errors
/// - [`AuthError::Identity`] — storage error or user not found.
pub async fn force_logout(
    user_repo: &dyn UserRepository,
    session_repo: &dyn SessionRepository,
    user_id: UserId,
) -> Result<(), AuthError> {
    user_repo
        .find_by_id(user_id)
        .await?
        .ok_or(AuthError::Identity(IdentityError::UserNotFound))?;
    session_repo.revoke_all_for_user(user_id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

    /// `create_if_empty_called` is the regression signal for the
    /// DS-AUD-035 guard: it must stay false when `count > 0`.
    struct MockUserRepo {
        count_value: AtomicI64,
        create_succeeds: bool,
        create_if_empty_called: AtomicBool,
    }

    impl MockUserRepo {
        fn new(count: i64, create_succeeds: bool) -> Self {
            Self {
                count_value: AtomicI64::new(count),
                create_succeeds,
                create_if_empty_called: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl UserRepository for MockUserRepo {
        async fn create(&self, _user: &User) -> Result<(), IdentityError> {
            Err(IdentityError::UsernameExists)
        }
        async fn find_by_id(&self, _id: UserId) -> Result<Option<User>, IdentityError> {
            Ok(None)
        }
        async fn find_by_username(&self, _username: &str) -> Result<Option<User>, IdentityError> {
            Ok(None)
        }
        async fn count(&self) -> Result<i64, IdentityError> {
            Ok(self.count_value.load(Ordering::Relaxed))
        }
        async fn list(
            &self,
            _cursor: Option<UserId>,
            _limit: u32,
        ) -> Result<Vec<User>, IdentityError> {
            Ok(Vec::new())
        }
        async fn create_if_empty(&self, _user: &User) -> Result<(), IdentityError> {
            self.create_if_empty_called.store(true, Ordering::Relaxed);
            if self.create_succeeds {
                Ok(())
            } else {
                Err(IdentityError::AlreadyInitialized)
            }
        }
        async fn set_enabled(&self, _id: UserId, _enabled: bool) -> Result<(), IdentityError> {
            Ok(())
        }
        async fn set_two_factor_enabled(
            &self,
            _id: UserId,
            _enabled: bool,
        ) -> Result<(), IdentityError> {
            Ok(())
        }
        async fn update_last_login(
            &self,
            _id: UserId,
            _at: deve_sub_kernel::Timestamp,
        ) -> Result<(), IdentityError> {
            Ok(())
        }
    }

    /// DS-AUD-035: when users already exist, setup_admin must reject
    /// before reaching create_if_empty.
    #[tokio::test]
    async fn setup_admin_rejects_when_initialized() {
        let repo = MockUserRepo::new(1, false);
        let result = setup_admin(&repo, "admin", "s3cure-pwd!").await;
        assert!(matches!(result, Err(AuthError::AlreadyInitialized)));
        assert!(
            !repo.create_if_empty_called.load(Ordering::Relaxed),
            "create_if_empty must not be called when count > 0"
        );
    }

    /// DS-AUD-035: when no users exist, setup_admin proceeds and
    /// create_if_empty is invoked.
    #[tokio::test]
    async fn setup_admin_proceeds_when_empty() {
        let repo = MockUserRepo::new(0, true);
        let result = setup_admin(&repo, "admin", "s3cure-pwd!").await;
        assert!(result.is_ok(), "setup_admin on empty DB should succeed");
        assert!(
            repo.create_if_empty_called.load(Ordering::Relaxed),
            "create_if_empty must be called when count == 0"
        );
    }
}
