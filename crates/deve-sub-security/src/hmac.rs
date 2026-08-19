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

/// HMAC purpose for node identity fingerprinting (B-12).
///
/// WHY: domain separation ensures a node identity digest cannot be replayed
/// as a session token hash or recovery code hash, and vice versa. The `v1`
/// suffix allows the canonical identity schema to evolve in the future
/// without colliding with fingerprints computed under an older schema.
pub const PURPOSE_NODE_IDENTITY: &str = "node-identity-v1";

/// Compute a node identity fingerprint (B-12).
///
/// When `key` is `Some`, returns [`hmac_digest`] keyed with the master key —
/// a keyed HMAC-SHA256 digest that prevents credential leakage via the
/// fingerprint column if the database is compromised.
///
/// When `key` is `None` (test mode without a master key), returns a plain
/// SHA256 digest of `purpose || ":" || value`. The two forms are NOT
/// interchangeable — a database populated under one form will not
/// deduplicate against the other — but within a single database instance the
/// form is consistent because the master key presence is fixed at startup.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if HMAC computation fails.
pub fn identity_fingerprint(
    purpose: &str,
    value: &str,
    key: Option<&[u8]>,
) -> Result<String, SecurityError> {
    match key {
        Some(k) => hmac_digest(purpose, value, k),
        None => {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(purpose.as_bytes());
            hasher.update(b":");
            hasher.update(value.as_bytes());
            Ok(URL_SAFE_NO_PAD.encode(hasher.finalize()))
        }
    }
}

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
