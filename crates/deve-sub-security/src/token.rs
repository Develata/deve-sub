//! Session token generation and HMAC-SHA256 hashing.
//!
//! Session tokens are CSPRNG-generated random bytes, base64url-encoded for
//! transport as a cookie value. The database stores only the HMAC-SHA256
//! digest of the token, computed with the server's master key. This ensures
//! that a database compromise alone cannot forge sessions. See
//! `docs/plan/00-engineering-constitution.md` §"Data and security".

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::Sha256;

use crate::SecurityError;

type HmacSha256 = Hmac<Sha256>;

/// Number of random bytes in a session token (256 bits of entropy).
const TOKEN_BYTES: usize = 32;

/// Generate a cryptographically secure session token.
///
/// Returns a base64url-no-pad encoded string of 32 random bytes
/// (43 characters, 256 bits of entropy).
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if the OS entropy source fails.
pub fn generate_session_token() -> Result<String, SecurityError> {
    let mut bytes = [0u8; TOKEN_BYTES];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|e| SecurityError::Crypto(format!("entropy source failure: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Compute the HMAC-SHA256 digest of a token using the master key.
///
/// Returns a base64url-no-pad encoded string. This digest is stored in the
/// database; the raw token is never persisted.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if HMAC computation fails (e.g. key
/// length is zero, which should not happen with a properly loaded master key).
pub fn hash_session_token(token: &str, key: &[u8]) -> Result<String, SecurityError> {
    let mut mac =
        HmacSha256::new_from_slice(key).map_err(|e| SecurityError::Crypto(e.to_string()))?;
    mac.update(token.as_bytes());
    let result = mac.finalize().into_bytes();
    Ok(URL_SAFE_NO_PAD.encode(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_43_chars() {
        let token = generate_session_token().expect("token generation");
        assert_eq!(token.len(), 43);
    }

    #[test]
    fn tokens_are_unique() {
        let a = generate_session_token().expect("token generation");
        let b = generate_session_token().expect("token generation");
        assert_ne!(a, b);
    }

    #[test]
    fn token_hash_is_deterministic() {
        let key = b"test-master-key-32-bytes-long!!!";
        let token = generate_session_token().expect("token generation");
        let hash1 = hash_session_token(&token, key).expect("hash");
        let hash2 = hash_session_token(&token, key).expect("hash");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn token_hash_changes_with_key() {
        let token = generate_session_token().expect("token generation");
        let hash1 = hash_session_token(&token, b"key-one-32-bytes-long!!!!!!!!").expect("hash");
        let hash2 = hash_session_token(&token, b"key-two-32-bytes-long!!!!!!!!").expect("hash");
        assert_ne!(hash1, hash2);
    }
}
