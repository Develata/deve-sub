//! Versioned secret envelope for at-rest encryption.
//!
//! The envelope format is `v1:{ciphertext_b64url}:{nonce_b64url}`, where the
//! ciphertext and nonce are produced by XChaCha20-Poly1305. The `v1:` prefix
//! enables future algorithm migration without ambiguity. See ADR-0007.
//!
//! Repository adapters call [`seal`] before writing sensitive columns and
//! [`open`] after reading them. The domain layer handles plaintext only in
//! memory.

use crate::{SecurityError, decrypt_from_b64, encrypt_to_b64};

/// Envelope version prefix. Used to distinguish envelope strings from
/// plaintext or legacy formats.
pub const ENVELOPE_PREFIX: &str = "v1:";

/// Separator between ciphertext and nonce in the envelope.
const SEPARATOR: char = ':';

/// Encrypt plaintext and return a versioned envelope string.
///
/// The envelope format is `v1:{ciphertext_b64url}:{nonce_b64url}`.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if encryption fails.
pub fn seal(key: &[u8], plaintext: &[u8]) -> Result<String, SecurityError> {
    let (ct, nonce) = encrypt_to_b64(key, plaintext)?;
    Ok(format!("{ENVELOPE_PREFIX}{ct}{SEPARATOR}{nonce}"))
}

/// Decrypt a versioned envelope string.
///
/// Returns the plaintext bytes. If the input does not start with `v1:`, it
/// is treated as a legacy unversioned envelope (`{ct}:{nonce}`) for backward
/// compatibility with probe adapter auth tokens. If the input has no
/// separator at all, [`SecurityError::Crypto`] is returned.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if the envelope is malformed or
/// decryption fails.
pub fn open(key: &[u8], envelope: &str) -> Result<Vec<u8>, SecurityError> {
    let body = if let Some(rest) = envelope.strip_prefix(ENVELOPE_PREFIX) {
        rest
    } else {
        // WHY: probe adapter auth tokens use the legacy unversioned format
        // `{ct}:{nonce}`. Accept them here so a single decrypt function
        // serves both old and new callers. Once all callers migrate to
        // `seal`, this fallback can be removed.
        envelope
    };

    let (ct, nonce) = body
        .split_once(SEPARATOR)
        .ok_or_else(|| SecurityError::Crypto("envelope missing separator".to_owned()))?;

    decrypt_from_b64(key, ct, nonce)
}

/// Check whether a string is a versioned envelope (starts with `v1:`).
///
/// Useful for distinguishing encrypted columns from plaintext during the
/// migration window.
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
    // WHY: parse rather than regex to correctly handle userinfo, port, and
    // IPv6 brackets without reinventing URL grammar.
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

    #[test]
    fn seal_open_roundtrip() {
        let plaintext = b"https://user:secret@host/path";
        let envelope = seal(&KEY, plaintext).expect("seal");
        assert!(is_envelope(&envelope));
        let recovered = open(&KEY, &envelope).expect("open");
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn envelope_has_v1_prefix() {
        let envelope = seal(&KEY, b"test").expect("seal");
        assert!(envelope.starts_with("v1:"));
        assert!(envelope.matches(':').count() >= 2);
    }

    #[test]
    fn wrong_key_fails() {
        let wrong_key = [0x99u8; 32];
        let envelope = seal(&KEY, b"secret").expect("seal");
        assert!(open(&wrong_key, &envelope).is_err());
    }

    #[test]
    fn malformed_envelope_fails() {
        assert!(open(&KEY, "not-an-envelope").is_err());
        assert!(open(&KEY, "v1:noseparator").is_err());
        assert!(open(&KEY, "v1:").is_err());
    }

    #[test]
    fn legacy_envelope_compat() {
        // WHY: probe adapters produce `{ct}:{nonce}` without the v1 prefix.
        // `open` must still decrypt these until all callers migrate.
        let (ct, nonce) = encrypt_to_b64(&KEY, b"legacy-secret").expect("encrypt");
        let legacy = format!("{ct}{SEPARATOR}{nonce}");
        assert!(!is_envelope(&legacy));
        let recovered = open(&KEY, &legacy).expect("open legacy");
        assert_eq!(recovered, b"legacy-secret");
    }

    #[test]
    fn ciphertext_does_not_contain_plaintext() {
        let plaintext = b"https://user:password@host:8080/path";
        let envelope = seal(&KEY, plaintext).expect("seal");
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
