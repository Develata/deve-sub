//! Stateless 2FA challenge token.
//!
//! After password verification succeeds but before 2FA verification, the
//! server issues a short-lived signed token that binds the login attempt to a
//! specific user. The token is stateless (no DB write): it encodes the user ID
//! and an expiry timestamp, signed with the master key via HMAC-SHA256.
//!
//! Format: `base64url(payload_json).base64url(hmac_signature)`
//!
//! The token is returned in the login JSON response (not a cookie) and sent
//! back by the client in the `POST /api/v1/auth/login/2fa` request body.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use deve_sub_kernel::{Timestamp, UserId};
use deve_sub_security::{MasterKey, hash_session_token};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use super::error::AuthError;

/// Challenge token lifetime in seconds (5 minutes).
const CHALLENGE_TTL_SECS: i64 = 300;

/// JSON payload encoded inside the challenge token.
#[derive(Serialize, Deserialize)]
struct ChallengePayload {
    uid: String,
    exp: i64,
}

/// Generate a 2FA challenge token for the given user ID.
///
/// The token expires after [`CHALLENGE_TTL_SECS`] seconds. It is signed with
/// the master key to prevent forgery.
///
/// # Errors
/// Returns [`AuthError::Security`] if token generation or signing fails.
pub fn generate_challenge_token(
    user_id: UserId,
    master_key: &MasterKey,
) -> Result<String, AuthError> {
    let exp = Timestamp::now() + time::Duration::seconds(CHALLENGE_TTL_SECS);
    let payload = ChallengePayload {
        uid: user_id.to_string(),
        exp: exp.as_offset_date_time().unix_timestamp(),
    };
    let payload_json = serde_json::to_string(&payload).map_err(|e| {
        AuthError::Security(deve_sub_security::SecurityError::Crypto(e.to_string()))
    })?;
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload_json.as_bytes());
    let signature = hash_session_token(&payload_b64, master_key.as_bytes())?;
    Ok(format!("{payload_b64}.{signature}"))
}

/// Verify a 2FA challenge token and extract the user ID.
///
/// Returns `Ok(Some(user_id))` if the signature is valid and the token has not
/// expired. Returns `Ok(None)` if the token is invalid, expired, or
/// malformed.
///
/// # Errors
/// Returns [`AuthError::Security`] only if an internal crypto operation fails
/// (not for invalid tokens — those return `Ok(None)`).
pub fn verify_challenge_token(
    token: &str,
    master_key: &MasterKey,
) -> Result<Option<UserId>, AuthError> {
    let Some((payload_b64, signature)) = token.rsplit_once('.') else {
        return Ok(None);
    };

    // WHY: recompute the signature and compare in constant time to prevent
    // timing side-channel attacks on the signature verification.
    let expected_signature = hash_session_token(payload_b64, master_key.as_bytes())?;
    let sig_bytes = signature.as_bytes();
    let expected_bytes = expected_signature.as_bytes();
    if sig_bytes.len() != expected_bytes.len() || !bool::from(sig_bytes.ct_eq(expected_bytes)) {
        return Ok(None);
    }

    let payload_bytes = URL_SAFE_NO_PAD.decode(payload_b64).map_err(|e| {
        AuthError::Security(deve_sub_security::SecurityError::Crypto(e.to_string()))
    })?;
    let payload: ChallengePayload = serde_json::from_slice(&payload_bytes).map_err(|e| {
        AuthError::Security(deve_sub_security::SecurityError::Crypto(e.to_string()))
    })?;

    let now = Timestamp::now().as_offset_date_time().unix_timestamp();
    if payload.exp <= now {
        return Ok(None);
    }

    let user_id = UserId::parse(&payload.uid).map_err(|e| {
        AuthError::Security(deve_sub_security::SecurityError::Crypto(e.to_string()))
    })?;
    Ok(Some(user_id))
}
