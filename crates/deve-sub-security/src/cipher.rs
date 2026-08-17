//! XChaCha20-Poly1305 authenticated encryption for sensitive fields.
//!
//! Used to encrypt TOTP secrets at rest so a database compromise alone
//! cannot forge 2FA codes. The master key (32 bytes) is shared with
//! HMAC-SHA256 token hashing. See
//! `docs/plan/00-engineering-constitution.md` §"Data and security".

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use rand::RngCore;
use rand::rngs::OsRng;

use crate::SecurityError;

/// Nonce length for XChaCha20-Poly1305 (192 bits / 24 bytes).
pub const NONCE_LEN: usize = 24;

/// Generate a random 24-byte nonce for XChaCha20-Poly1305.
fn generate_nonce() -> Result<[u8; NONCE_LEN], SecurityError> {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|e| SecurityError::Crypto(format!("nonce generation failed: {e}")))?;
    Ok(nonce)
}

/// Encrypt plaintext using XChaCha20-Poly1305 with the given 32-byte key.
///
/// Returns the ciphertext (which includes the 16-byte Poly1305 tag) and the
/// 24-byte nonce. Both must be stored; decryption requires both.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if encryption fails.
pub fn encrypt(key: &[u8], plaintext: &[u8]) -> Result<(Vec<u8>, [u8; NONCE_LEN]), SecurityError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| SecurityError::Crypto(format!("invalid key length: {e}")))?;
    let nonce = generate_nonce()?;
    let ciphertext = cipher
        .encrypt((&nonce).into(), plaintext)
        .map_err(|e| SecurityError::Crypto(format!("encryption failed: {e}")))?;
    Ok((ciphertext, nonce))
}

/// Decrypt ciphertext using XChaCha20-Poly1305 with the given key and nonce.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if decryption fails (wrong key, corrupted
/// ciphertext, or tampered tag).
pub fn decrypt(key: &[u8], ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>, SecurityError> {
    if nonce.len() != NONCE_LEN {
        return Err(SecurityError::Crypto(format!(
            "invalid nonce length: expected {NONCE_LEN}, got {}",
            nonce.len()
        )));
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| SecurityError::Crypto(format!("invalid key length: {e}")))?;
    let nonce: &[u8; NONCE_LEN] = nonce
        .try_into()
        .map_err(|_| SecurityError::Crypto("nonce conversion failed".to_owned()))?;
    cipher
        .decrypt(nonce.into(), ciphertext)
        .map_err(|e| SecurityError::Crypto(format!("decryption failed: {e}")))
}

/// Encrypt and encode as base64url strings for convenient TEXT storage.
///
/// Returns `(ciphertext_b64, nonce_b64)`.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if encryption fails.
pub fn encrypt_to_b64(key: &[u8], plaintext: &[u8]) -> Result<(String, String), SecurityError> {
    let (ciphertext, nonce) = encrypt(key, plaintext)?;
    Ok((encode_b64(&ciphertext), encode_b64(&nonce)))
}

/// Encode bytes as base64url (no padding).
#[must_use]
pub fn encode_b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Decode a base64url (no padding) string.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if the input is not valid base64url.
pub fn decode_b64(s: &str) -> Result<Vec<u8>, SecurityError> {
    URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|e| SecurityError::Crypto(format!("base64 decode failed: {e}")))
}

/// Encrypt plaintext with additional authenticated data (AAD).
///
/// The AAD is authenticated but not encrypted — it binds the ciphertext to a
/// specific context (e.g. a table+column identifier) so a ciphertext copied
/// from one column cannot be decrypted in another. Returns the ciphertext
/// (with Poly1305 tag) and the 24-byte nonce.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if encryption fails.
pub fn encrypt_aad(
    key: &[u8],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(Vec<u8>, [u8; NONCE_LEN]), SecurityError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| SecurityError::Crypto(format!("invalid key length: {e}")))?;
    let nonce = generate_nonce()?;
    let ciphertext = cipher
        .encrypt(
            (&nonce).into(),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|e| SecurityError::Crypto(format!("encryption failed: {e}")))?;
    Ok((ciphertext, nonce))
}

/// Decrypt ciphertext with additional authenticated data (AAD).
///
/// The AAD must match the value used during encryption; otherwise decryption
/// fails (Poly1305 tag mismatch).
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if decryption fails (wrong key, corrupted
/// ciphertext, tampered tag, or AAD mismatch).
pub fn decrypt_aad(
    key: &[u8],
    ciphertext: &[u8],
    nonce: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, SecurityError> {
    if nonce.len() != NONCE_LEN {
        return Err(SecurityError::Crypto(format!(
            "invalid nonce length: expected {NONCE_LEN}, got {}",
            nonce.len()
        )));
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| SecurityError::Crypto(format!("invalid key length: {e}")))?;
    let nonce: &[u8; NONCE_LEN] = nonce
        .try_into()
        .map_err(|_| SecurityError::Crypto("nonce conversion failed".to_owned()))?;
    cipher
        .decrypt(
            nonce.into(),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|e| SecurityError::Crypto(format!("decryption failed: {e}")))
}

/// Decrypt from base64url-encoded strings.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if decoding or decryption fails.
pub fn decrypt_from_b64(
    key: &[u8],
    ciphertext_b64: &str,
    nonce_b64: &str,
) -> Result<Vec<u8>, SecurityError> {
    let ciphertext = decode_b64(ciphertext_b64)?;
    let nonce = decode_b64(nonce_b64)?;
    decrypt(key, &ciphertext, &nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = [0x42u8; 32];
        let plaintext = b"secret-totp-data";
        let (ciphertext, nonce) = encrypt(&key, plaintext).expect("encrypt");
        let decrypted = decrypt(&key, &ciphertext, &nonce).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn ciphertext_differs_from_plaintext() {
        let key = [0x42u8; 32];
        let plaintext = b"secret-totp-data";
        let (ciphertext, _) = encrypt(&key, plaintext).expect("encrypt");
        assert_ne!(&ciphertext[..], plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let key = [0x42u8; 32];
        let wrong_key = [0x99u8; 32];
        let (ciphertext, nonce) = encrypt(&key, b"secret").expect("encrypt");
        assert!(decrypt(&wrong_key, &ciphertext, &nonce).is_err());
    }

    #[test]
    fn b64_roundtrip() {
        let key = [0x42u8; 32];
        let plaintext = b"totp-secret-bytes";
        let (ct_b64, n_b64) = encrypt_to_b64(&key, plaintext).expect("encrypt");
        let decrypted = decrypt_from_b64(&key, &ct_b64, &n_b64).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn aad_roundtrip() {
        let key = [0x42u8; 32];
        let aad = b"sources.url";
        let (ct, nonce) = encrypt_aad(&key, b"secret", aad).expect("encrypt");
        let pt = decrypt_aad(&key, &ct, &nonce, aad).expect("decrypt");
        assert_eq!(pt, b"secret");
    }

    #[test]
    fn aad_mismatch_fails() {
        let key = [0x42u8; 32];
        let (ct, nonce) = encrypt_aad(&key, b"secret", b"sources.url").expect("encrypt");
        assert!(
            decrypt_aad(&key, &ct, &nonce, b"sources.headers").is_err(),
            "AAD mismatch must fail decryption"
        );
    }

    #[test]
    fn aad_wrong_key_fails() {
        let key = [0x42u8; 32];
        let wrong = [0x99u8; 32];
        let (ct, nonce) = encrypt_aad(&key, b"secret", b"ctx").expect("encrypt");
        assert!(decrypt_aad(&wrong, &ct, &nonce, b"ctx").is_err());
    }
}
