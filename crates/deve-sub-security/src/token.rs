//! Session token generation.
//!
//! Session tokens are CSPRNG-generated random bytes, base64url-encoded for
//! transport as a cookie value. The database stores only the HMAC-SHA256
//! digest of the token (computed via [`crate::hmac_digest`]), not the raw
//! token. This ensures that a database compromise alone cannot forge
//! sessions. See `docs/plan/00-engineering-constitution.md` §"Data and
//! security".

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use rand::rngs::OsRng;

use crate::SecurityError;

/// Number of random bytes in a session token (256 bits of entropy).
const TOKEN_BYTES: usize = 32;

/// Base62 alphabet for short codes (URL-safe, no ambiguous chars excluded;
/// entropy comes from the CSPRNG, not the alphabet).
const BASE62_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Number of characters in a short code. 8 chars × log2(62) ≈ 47.6 bits of
/// entropy — sufficient for a public lookup key with UNIQUE-constraint retry
/// (OUT-013). See M6 blueprint §"Token and short-code security model".
const SHORT_CODE_LEN: usize = 8;

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

/// Generate a cryptographically secure short code.
///
/// Returns an 8-character base62 string (≥47 bits of entropy). Unlike session
/// tokens, short codes are stored in the clear — they are public lookup keys
/// for `GET /s/{code}`, not secrets. The caller retries on UNIQUE conflict
/// (OUT-013).
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if the OS entropy source fails.
pub fn generate_short_code() -> Result<String, SecurityError> {
    let mut idx = [0u8; SHORT_CODE_LEN];
    OsRng
        .try_fill_bytes(&mut idx)
        .map_err(|e| SecurityError::Crypto(format!("entropy source failure: {e}")))?;
    let code: String = idx
        .iter()
        .map(|b| BASE62_ALPHABET[(*b % 62) as usize] as char)
        .collect();
    Ok(code)
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
    fn short_code_is_8_chars() {
        let code = generate_short_code().expect("short code generation");
        assert_eq!(code.len(), SHORT_CODE_LEN);
    }

    #[test]
    fn short_code_is_base62() {
        let code = generate_short_code().expect("short code generation");
        assert!(code.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn short_codes_are_unique() {
        let a = generate_short_code().expect("short code generation");
        let b = generate_short_code().expect("short code generation");
        assert_ne!(a, b);
    }
}
