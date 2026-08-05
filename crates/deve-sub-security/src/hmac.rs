//! Generic HMAC-SHA256 digest with domain separation.
//!
//! This module provides [`hmac_digest`], a purpose-tagged HMAC function used
//! across the application: session token hashing, recovery code hashing, and
//! 2FA challenge token signing. Each call site passes a `purpose` string that
//! is mixed into the HMAC input, preventing cross-protocol reuse of a digest
//! (e.g. a session token hash cannot be replayed as a recovery code hash).
//! See `docs/plan/00-engineering-constitution.md` §"Data and security".

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::SecurityError;

type HmacSha256 = Hmac<Sha256>;

/// HMAC purpose for session token hashing.
pub const PURPOSE_SESSION: &str = "session";

/// HMAC purpose for recovery code hashing.
pub const PURPOSE_RECOVERY: &str = "recovery";

/// HMAC purpose for 2FA challenge token signing.
pub const PURPOSE_CHALLENGE: &str = "challenge";

/// Compute a domain-separated HMAC-SHA256 digest.
///
/// The `purpose` string is prepended to `value` in the HMAC input as
/// `purpose ":" value`, ensuring that a digest from one context (e.g.
/// `"session"`) cannot be valid in another (e.g. `"recovery"`). The result is
/// base64url-no-pad encoded for database storage.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if HMAC computation fails (e.g. key
/// length is zero, which should not happen with a properly loaded master key).
pub fn hmac_digest(purpose: &str, value: &str, key: &[u8]) -> Result<String, SecurityError> {
    let mut mac =
        HmacSha256::new_from_slice(key).map_err(|e| SecurityError::Crypto(e.to_string()))?;
    mac.update(purpose.as_bytes());
    mac.update(b":");
    mac.update(value.as_bytes());
    let result = mac.finalize().into_bytes();
    Ok(URL_SAFE_NO_PAD.encode(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"test-master-key-32-bytes-long!!!";

    #[test]
    fn digest_is_deterministic() {
        let a = hmac_digest(PURPOSE_SESSION, "abc", KEY).expect("hash");
        let b = hmac_digest(PURPOSE_SESSION, "abc", KEY).expect("hash");
        assert_eq!(a, b);
    }

    #[test]
    fn digest_changes_with_purpose() {
        // WHY: domain separation — the same value under different purposes
        // must produce different digests, preventing cross-protocol replay.
        let session = hmac_digest(PURPOSE_SESSION, "secret", KEY).expect("hash");
        let recovery = hmac_digest(PURPOSE_RECOVERY, "secret", KEY).expect("hash");
        let challenge = hmac_digest(PURPOSE_CHALLENGE, "secret", KEY).expect("hash");
        assert_ne!(session, recovery);
        assert_ne!(session, challenge);
        assert_ne!(recovery, challenge);
    }

    #[test]
    fn digest_changes_with_key() {
        let a = hmac_digest(PURPOSE_SESSION, "abc", KEY).expect("hash");
        let b =
            hmac_digest(PURPOSE_SESSION, "abc", b"other-key-32-bytes-long!!!!!!").expect("hash");
        assert_ne!(a, b);
    }
}
