//! HKDF-SHA256 subkey derivation for the secret envelope.
//!
//! Each envelope operation derives a 32-byte subkey from the master key via
//! HKDF-SHA256 (RFC 5869). This separates the envelope key material from the
//! master key's other uses (HMAC-SHA256 for session tokens and key
//! fingerprints) so that a compromise or weakness in one consumer cannot
//! affect the others.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::SecurityError;

type HmacSha256 = Hmac<Sha256>;

/// HKDF-Extract: PRK = HMAC-SHA256(salt, IKM).
fn extract(salt: &[u8], ikm: &[u8]) -> Result<[u8; 32], SecurityError> {
    let mut mac = HmacSha256::new_from_slice(salt)
        .map_err(|e| SecurityError::Crypto(format!("hkdf extract: {e}")))?;
    mac.update(ikm);
    let prk = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&prk);
    Ok(out)
}

/// HKDF-Expand: OKM = HMAC-SHA256(PRK, info || 0x01) for L = 32 (single block).
fn expand(prk: &[u8; 32], info: &[u8]) -> Result<[u8; 32], SecurityError> {
    let mut mac = HmacSha256::new_from_slice(prk)
        .map_err(|e| SecurityError::Crypto(format!("hkdf expand: {e}")))?;
    mac.update(info);
    mac.update(&[0x01]);
    let okm = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&okm);
    Ok(out)
}

/// Derive a 32-byte envelope subkey from the master key and a context label.
///
/// The context label (e.g. `b"sources.url"`) serves double duty: it is the
/// HKDF `info` parameter (producing a unique subkey per column) and the AAD
/// passed to XChaCha20-Poly1305 (binding the ciphertext to its column so a
/// ciphertext relocated to another column fails to decrypt).
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if the HMAC primitive rejects the key
/// length (cannot happen for a 32-byte PRK, but the error path is preserved
/// for completeness).
pub fn derive_envelope_subkey(
    master_key: &[u8],
    context: &[u8],
) -> Result<[u8; 32], SecurityError> {
    const SALTS: &[u8] = b"deve-sub-envelope-v2";
    let prk = extract(SALTS, master_key)?;
    expand(&prk, context)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [0x42u8; 32];

    #[test]
    fn different_contexts_yield_different_subkeys() {
        let k1 = derive_envelope_subkey(&KEY, b"sources.url").expect("derive");
        let k2 = derive_envelope_subkey(&KEY, b"sources.headers").expect("derive");
        assert_ne!(k1, k2, "different contexts must produce different subkeys");
    }

    #[test]
    fn same_context_is_deterministic() {
        let k1 = derive_envelope_subkey(&KEY, b"nodes.tls_json").expect("derive");
        let k2 = derive_envelope_subkey(&KEY, b"nodes.tls_json").expect("derive");
        assert_eq!(k1, k2);
    }

    #[test]
    fn different_master_keys_yield_different_subkeys() {
        let other_key = [0x99u8; 32];
        let k1 = derive_envelope_subkey(&KEY, b"sources.url").expect("derive");
        let k2 = derive_envelope_subkey(&other_key, b"sources.url").expect("derive");
        assert_ne!(k1, k2);
    }

    #[test]
    fn subkey_differs_from_master_key() {
        let sub = derive_envelope_subkey(&KEY, b"sources.url").expect("derive");
        assert_ne!(sub, KEY, "subkey must not equal the master key");
    }
}
