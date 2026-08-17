//! Versioned secret envelope for at-rest encryption.
//!
//! The envelope format is `v2:{ciphertext_b64url}:{nonce_b64url}`. The
//! ciphertext and nonce are produced by XChaCha20-Poly1305 with
//! HKDF-SHA256-derived subkeys and column-bound AAD. See ADR-0007.
//!
//! Each caller passes a stable `context` label (e.g. `b"sources.url"`) that
//! serves as both the HKDF `info` parameter (yielding a unique subkey per
//! column) and the AEAD AAD (binding the ciphertext to its column so a
//! ciphertext relocated across columns fails to decrypt).
//!
//! Repository adapters call [`seal`] before writing sensitive columns and
//! [`open`] after reading them. The domain layer handles plaintext only in
//! memory.

use crate::{SecurityError, decrypt_aad, derive_envelope_subkey, encrypt_aad};

/// Envelope version prefix.
pub const ENVELOPE_PREFIX: &str = "v2:";

/// Separator between ciphertext and nonce in the envelope.
const SEPARATOR: char = ':';

/// Encrypt plaintext and return a versioned envelope string.
///
/// The `context` label derives a column-specific subkey via HKDF-SHA256 and
/// is bound to the ciphertext as AAD. The envelope format is
/// `v2:{ciphertext_b64url}:{nonce_b64url}`.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if subkey derivation or encryption fails.
pub fn seal(master_key: &[u8], context: &[u8], plaintext: &[u8]) -> Result<String, SecurityError> {
    let subkey = derive_envelope_subkey(master_key, context)?;
    let (ct, nonce) = encrypt_aad(&subkey, plaintext, context)?;
    Ok(format!(
        "{ENVELOPE_PREFIX}{}{SEPARATOR}{}",
        crate::cipher::encode_b64(&ct),
        crate::cipher::encode_b64(&nonce)
    ))
}

/// Decrypt a versioned envelope string.
///
/// The `context` label must match the one used by [`seal`]; a mismatch fails
/// decryption (Poly1305 tag mismatch) because the AAD differs.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if the envelope is malformed, the prefix
/// is not `v2:`, or decryption fails.
pub fn open(master_key: &[u8], context: &[u8], envelope: &str) -> Result<Vec<u8>, SecurityError> {
    let body = envelope
        .strip_prefix(ENVELOPE_PREFIX)
        .ok_or_else(|| SecurityError::Crypto("envelope missing v2 prefix".to_owned()))?;
    let (ct_b64, nonce_b64) = body
        .split_once(SEPARATOR)
        .ok_or_else(|| SecurityError::Crypto("envelope missing separator".to_owned()))?;
    let subkey = derive_envelope_subkey(master_key, context)?;
    let ct = crate::cipher::decode_b64(ct_b64)?;
    let nonce = crate::cipher::decode_b64(nonce_b64)?;
    decrypt_aad(&subkey, &ct, &nonce, context)
}

/// Check whether a string is a versioned envelope (starts with `v2:`).
#[must_use]
pub fn is_envelope(s: &str) -> bool {
    s.starts_with(ENVELOPE_PREFIX)
}

/// Mask a URL for display: show the scheme and host, redact the rest.
///
/// `https://user:pass@host:8080/path?query` becomes `https://host:8080/***`.
/// If the URL cannot be parsed, the entire value is replaced with `***`.
#[must_use]
pub fn mask_url(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return "***".to_owned();
    };
    let scheme = parsed.scheme();
    let host = match parsed.host_str() {
        Some(h) => h,
        None => return "***".to_owned(),
    };
    if let Some(port) = parsed.port() {
        format!("{scheme}://{host}:{port}/***")
    } else {
        format!("{scheme}://{host}/***")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [0x42u8; 32];
    const CTX: &[u8] = b"sources.url";

    #[test]
    fn seal_open_roundtrip() {
        let plaintext = b"https://user:secret@host/path";
        let envelope = seal(&KEY, CTX, plaintext).expect("seal");
        assert!(is_envelope(&envelope));
        let recovered = open(&KEY, CTX, &envelope).expect("open");
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn envelope_has_v2_prefix() {
        let envelope = seal(&KEY, CTX, b"test").expect("seal");
        assert!(envelope.starts_with("v2:"));
        assert!(envelope.matches(':').count() >= 2);
    }

    #[test]
    fn wrong_key_fails() {
        let wrong_key = [0x99u8; 32];
        let envelope = seal(&KEY, CTX, b"secret").expect("seal");
        assert!(open(&wrong_key, CTX, &envelope).is_err());
    }

    #[test]
    fn wrong_context_fails() {
        let envelope = seal(&KEY, CTX, b"secret").expect("seal");
        assert!(
            open(&KEY, b"sources.headers", &envelope).is_err(),
            "ciphertext bound to one context must not decrypt under another"
        );
    }

    #[test]
    fn malformed_envelope_fails() {
        assert!(open(&KEY, CTX, "not-an-envelope").is_err());
        assert!(open(&KEY, CTX, "v2:noseparator").is_err());
        assert!(open(&KEY, CTX, "v2:").is_err());
    }

    #[test]
    fn rejects_v1_prefix() {
        assert!(
            open(&KEY, CTX, "v1:abc:def").is_err(),
            "v1 envelopes are no longer supported"
        );
    }

    #[test]
    fn ciphertext_does_not_contain_plaintext() {
        let plaintext = b"https://user:password@host:8080/path";
        let envelope = seal(&KEY, CTX, plaintext).expect("seal");
        let plaintext_str = std::str::from_utf8(plaintext).expect("utf8");
        assert!(!envelope.contains(plaintext_str));
        assert!(!envelope.contains("password"));
        assert!(!envelope.contains("host"));
    }

    #[test]
    fn mask_url_redacts_credentials_and_path() {
        let masked = mask_url("https://user:pass@host:8080/path?query=1");
        assert_eq!(masked, "https://host:8080/***");
        assert!(!masked.contains("user"));
        assert!(!masked.contains("pass"));
        assert!(!masked.contains("path"));
    }

    #[test]
    fn mask_url_without_port() {
        let masked = mask_url("https://host/path");
        assert_eq!(masked, "https://host/***");
    }

    #[test]
    fn mask_url_invalid_returns_redacted() {
        assert_eq!(mask_url("not a url"), "***");
        assert_eq!(mask_url(""), "***");
    }

    #[test]
    fn mask_url_preserves_scheme() {
        let masked = mask_url("ss://user:pass@host:8388");
        assert_eq!(masked, "ss://host:8388/***");
    }
}
