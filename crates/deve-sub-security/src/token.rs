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
}
