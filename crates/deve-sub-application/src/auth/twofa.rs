//! 2FA application commands: setup, verify, disable, regenerate recovery
//! codes, and 2FA login.
//!
//! These functions orchestrate domain services, security primitives, and port
//! interfaces. They do not execute SQL directly. One API operation maps to one
//! command. See `docs/plan/milestones/M2-auth-and-users.md` (Slice 4).

use deve_sub_domain::{
    IdentityError, RecoveryCode, RecoveryCodeRepository, Session, SessionRepository, TotpSecret,
    TotpSecretRepository, User, UserRepository,
};
use deve_sub_kernel::{Timestamp, UserId};
use deve_sub_security::{
    MasterKey, decrypt, encrypt, generate_recovery_codes, generate_session_token,
    hash_session_token, normalize_recovery_code, totp_generate_secret, totp_otpauth_uri,
    totp_verify_code, verify_password,
};

use super::challenge::verify_challenge_token;
use super::error::AuthError;
use super::rate_limiter::LoginRateLimiter;

/// The result of a successful [`setup_2fa`] call.
pub struct TwoFactorSetupResult {
    /// Base32-encoded TOTP secret for manual entry.
    pub secret: String,
    /// `otpauth://` URI for QR code generation.
    pub otpauth_uri: String,
}

/// Generate a new TOTP secret, encrypt it, and store it for the user.
///
/// The secret is NOT yet active — the user must call [`verify_2fa`] with a
/// valid TOTP code to enable 2FA. If the user already has a stored secret
/// (e.g. from a previous incomplete setup), it is replaced.
///
/// # Errors
/// - [`AuthError::TwoFactorAlreadyEnabled`] — 2FA is already active.
/// - [`AuthError::Security`] — encryption or secret generation failed.
/// - [`AuthError::Identity`] — storage error.
pub async fn setup_2fa(
    user_repo: &dyn UserRepository,
    totp_secret_repo: &dyn TotpSecretRepository,
    master_key: &MasterKey,
    user_id: UserId,
    issuer: &str,
) -> Result<TwoFactorSetupResult, AuthError> {
    let user = user_repo
        .find_by_id(user_id)
        .await?
        .ok_or(AuthError::InvalidCredentials)?;

    if user.two_factor_enabled {
        return Err(AuthError::TwoFactorAlreadyEnabled);
    }

    let secret = totp_generate_secret()?;
    let (ciphertext, nonce) = encrypt(master_key.as_bytes(), &secret)?;
    let totp_secret = TotpSecret::new(user_id, ciphertext, nonce.to_vec());
    totp_secret_repo.upsert(&totp_secret).await?;

    let secret_b32 = deve_sub_security::base32_secret(&secret);
    let otpauth_uri = totp_otpauth_uri(&secret, issuer, &user.username);

    Ok(TwoFactorSetupResult {
        secret: secret_b32,
        otpauth_uri,
    })
}

/// The result of a successful [`verify_2fa`] call.
pub struct TwoFactorVerifyResult {
    /// Single-use recovery codes (shown once to the user).
    pub recovery_codes: Vec<String>,
}

/// Verify a TOTP code, enable 2FA, and generate recovery codes.
///
/// The user must have called [`setup_2fa`] first. This function:
/// 1. Loads and decrypts the stored TOTP secret.
/// 2. Verifies the provided code against the secret.
/// 3. Generates 10 recovery codes and stores their hashes atomically.
/// 4. Sets `two_factor_enabled = true` on the user.
///
/// WHY: recovery codes are stored BEFORE enabling 2FA. If `replace_all_for_user`
/// fails, 2FA stays off and the user can retry without risk of lockout. If
/// `set_two_factor_enabled` fails after codes are stored, the orphaned codes
/// are eagerly deleted (see inline cleanup) and the error is propagated.
///
/// # Errors
/// - [`AuthError::TotpSecretNotFound`] — no TOTP secret stored (setup not done).
/// - [`AuthError::TwoFactorAlreadyEnabled`] — 2FA is already active.
/// - [`AuthError::InvalidTwoFactorCode`] — wrong TOTP code.
/// - [`AuthError::Security`] — decryption failed.
/// - [`AuthError::Identity`] — storage error.
pub async fn verify_2fa(
    user_repo: &dyn UserRepository,
    totp_secret_repo: &dyn TotpSecretRepository,
    recovery_code_repo: &dyn RecoveryCodeRepository,
    master_key: &MasterKey,
    user_id: UserId,
    code: u32,
) -> Result<TwoFactorVerifyResult, AuthError> {
    let user = user_repo
        .find_by_id(user_id)
        .await?
        .ok_or(AuthError::InvalidCredentials)?;

    if user.two_factor_enabled {
        return Err(AuthError::TwoFactorAlreadyEnabled);
    }

    let stored_secret = totp_secret_repo
        .find_by_user(user_id)
        .await?
        .ok_or(AuthError::TotpSecretNotFound)?;

    let plaintext = decrypt(
        master_key.as_bytes(),
        &stored_secret.secret_ciphertext,
        &stored_secret.nonce,
    )?;

    if !totp_verify_code(&plaintext, code) {
        return Err(AuthError::InvalidTwoFactorCode);
    }

    let recovery_codes = generate_recovery_codes()?;
    let mut code_entities = Vec::with_capacity(recovery_codes.len());
    for code in &recovery_codes {
        let normalized = normalize_recovery_code(code);
        let hash = hash_session_token(&normalized, master_key.as_bytes())?;
        code_entities.push(RecoveryCode::new(user_id, hash));
    }
    recovery_code_repo
        .replace_all_for_user(user_id, &code_entities)
        .await?;

    // WHY: if set_two_factor_enabled fails after codes are stored, eagerly
    // delete the orphaned codes so a codes-without-2FA inconsistency does
    // not persist if the user never retries. The codes are unusable without
    // two_factor_enabled=true, so this is a cleanliness measure, not a
    // security fix.
    if let Err(e) = user_repo.set_two_factor_enabled(user_id, true).await {
        let _ = recovery_code_repo.delete_all_for_user(user_id).await;
        return Err(AuthError::Identity(e));
    }

    Ok(TwoFactorVerifyResult { recovery_codes })
}

/// Disable 2FA for a user after re-authenticating with their password.
///
/// Deletes the TOTP secret and all recovery codes. The user must provide
/// their current password to prevent unauthorized disabling (e.g. from a
/// hijacked session).
///
/// # Errors
/// - [`AuthError::TwoFactorNotEnabled`] — 2FA is not enabled.
/// - [`AuthError::InvalidCredentials`] — wrong password.
/// - [`AuthError::Identity`] — storage error.
pub async fn disable_2fa(
    user_repo: &dyn UserRepository,
    totp_secret_repo: &dyn TotpSecretRepository,
    recovery_code_repo: &dyn RecoveryCodeRepository,
    user_id: UserId,
    password: &str,
) -> Result<(), AuthError> {
    let user = user_repo
        .find_by_id(user_id)
        .await?
        .ok_or(AuthError::InvalidCredentials)?;

    if !user.two_factor_enabled {
        return Err(AuthError::TwoFactorNotEnabled);
    }

    if !verify_password(password, &user.password_hash)? {
        return Err(AuthError::InvalidCredentials);
    }

    // WHY: three non-transactional writes. If `set_two_factor_enabled(false)`
    // succeeds but a subsequent delete fails, orphaned TOTP secret or recovery
    // codes may remain. This is safe because both are only queried when
    // `two_factor_enabled=true` (checked in `login` before routing to
    // `login_2fa`), so orphaned data is unreachable. On re-enable,
    // `setup_2fa` replaces the TOTP secret and `verify_2fa` calls
    // `replace_all_for_user` which cleans up old recovery codes.
    user_repo.set_two_factor_enabled(user_id, false).await?;
    totp_secret_repo.delete(user_id).await?;
    recovery_code_repo.delete_all_for_user(user_id).await?;

    Ok(())
}

/// Regenerate recovery codes after re-authenticating with the password.
///
/// Deletes all existing recovery codes and generates a new batch. The old
/// codes are immediately invalid.
///
/// # Errors
/// - [`AuthError::TwoFactorNotEnabled`] — 2FA is not enabled.
/// - [`AuthError::InvalidCredentials`] — wrong password.
/// - [`AuthError::Security`] — code generation or hashing failed.
/// - [`AuthError::Identity`] — storage error.
pub async fn regenerate_recovery_codes(
    user_repo: &dyn UserRepository,
    recovery_code_repo: &dyn RecoveryCodeRepository,
    master_key: &MasterKey,
    user_id: UserId,
    password: &str,
) -> Result<Vec<String>, AuthError> {
    let user = user_repo
        .find_by_id(user_id)
        .await?
        .ok_or(AuthError::InvalidCredentials)?;

    if !user.two_factor_enabled {
        return Err(AuthError::TwoFactorNotEnabled);
    }

    if !verify_password(password, &user.password_hash)? {
        return Err(AuthError::InvalidCredentials);
    }

    let recovery_codes = generate_recovery_codes()?;
    let mut code_entities = Vec::with_capacity(recovery_codes.len());
    for code in &recovery_codes {
        let normalized = normalize_recovery_code(code);
        let hash = hash_session_token(&normalized, master_key.as_bytes())?;
        code_entities.push(RecoveryCode::new(user_id, hash));
    }
    // WHY: replace_all_for_user atomically deletes old codes and inserts new
    // ones in a single transaction, eliminating the zero-codes window that
    // existed when delete and insert were separate calls (AUTH-006).
    recovery_code_repo
        .replace_all_for_user(user_id, &code_entities)
        .await?;

    Ok(recovery_codes)
}

/// Parameters for the [`login_2fa`] command.
pub struct LoginTwoFactorParams<'a> {
    pub user_repo: &'a dyn UserRepository,
    pub session_repo: &'a dyn SessionRepository,
    pub totp_secret_repo: &'a dyn TotpSecretRepository,
    pub recovery_code_repo: &'a dyn RecoveryCodeRepository,
    pub rate_limiter: &'a dyn LoginRateLimiter,
    pub master_key: &'a MasterKey,
    pub challenge_token: &'a str,
    pub code: &'a str,
    pub ip: Option<&'a str>,
    pub session_ttl: time::Duration,
}

/// Complete a 2FA login by verifying the challenge token and the TOTP code
/// or recovery code.
///
/// The `code` field is interpreted as a TOTP code if it is exactly 6 digits;
/// otherwise it is treated as a recovery code.
///
/// # Errors
/// - [`AuthError::ChallengeTokenInvalid`] — invalid or expired challenge token.
/// - [`AuthError::InvalidTwoFactorCode`] — wrong TOTP code or recovery code.
/// - [`AuthError::RateLimited`] — too many failed attempts (AUTH-004).
/// - [`AuthError::Security`] — crypto operation failed.
/// - [`AuthError::Identity`] — storage error.
pub async fn login_2fa(
    params: LoginTwoFactorParams<'_>,
) -> Result<(User, Session, String), AuthError> {
    let LoginTwoFactorParams {
        user_repo,
        session_repo,
        totp_secret_repo,
        recovery_code_repo,
        rate_limiter,
        master_key,
        challenge_token,
        code,
        ip,
        session_ttl,
    } = params;

    let user_id = verify_challenge_token(challenge_token, master_key)?
        .ok_or(AuthError::ChallengeTokenInvalid)?;

    let user = user_repo
        .find_by_id(user_id)
        .await?
        .ok_or(AuthError::ChallengeTokenInvalid)?;

    if !user.is_active() {
        return Err(AuthError::ChallengeTokenInvalid);
    }

    // WHY: rate-limit using the username so that TOTP failures accumulate in
    // the same counter as password failures. This prevents unlimited TOTP
    // brute-force attempts.
    rate_limiter.check(&user.username, ip)?;

    let code_valid = if code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()) {
        // TOTP code path
        let code_u32: u32 = code.parse().map_err(|_| AuthError::InvalidTwoFactorCode)?;

        let stored_secret = totp_secret_repo
            .find_by_user(user_id)
            .await?
            .ok_or(AuthError::TotpSecretNotFound)?;

        let plaintext = decrypt(
            master_key.as_bytes(),
            &stored_secret.secret_ciphertext,
            &stored_secret.nonce,
        )?;

        totp_verify_code(&plaintext, code_u32)
    } else {
        // Recovery code path
        let normalized = normalize_recovery_code(code);
        let hash = hash_session_token(&normalized, master_key.as_bytes())?;

        match recovery_code_repo
            .find_unused_by_hash(user_id, &hash)
            .await?
        {
            Some(recovery_code) => {
                // WHY: if mark_used fails with RecoveryCodeNotFound, another
                // concurrent request already consumed this code. Return
                // false to enforce single-use (AUTH-006).
                match recovery_code_repo.mark_used(recovery_code.id).await {
                    Ok(()) => true,
                    Err(IdentityError::RecoveryCodeNotFound) => false,
                    Err(other) => return Err(AuthError::Identity(other)),
                }
            }
            None => false,
        }
    };

    if !code_valid {
        rate_limiter.record_failure(&user.username, ip);
        return Err(AuthError::InvalidTwoFactorCode);
    }

    let now = Timestamp::now();
    let token = generate_session_token()?;
    let token_hash = hash_session_token(&token, master_key.as_bytes())?;
    let expires_at = now + session_ttl;
    let session = Session::new(user.id, token_hash, expires_at);
    session_repo.create(&session).await?;

    // WHY: update last_login_at on every successful 2FA login, matching the
    // non-2FA login path. `let _ =` ignores storage failure — the login
    // already succeeded; last_login_at is a cosmetic field, not a security
    // invariant.
    let _ = user_repo.update_last_login(user.id, now).await;

    rate_limiter.record_success(&user.username, ip);

    Ok((user, session, token))
}
