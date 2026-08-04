//! TOTP secret entity — stores the encrypted TOTP secret for a user.
//!
//! The domain holds the encrypted ciphertext and nonce; the application layer
//! handles encryption/decryption via [`deve_sub_security::cipher`]. This keeps
//! crypto concerns in the security crate and storage concerns in the adapter,
//! not in the domain.

use deve_sub_kernel::{Timestamp, UserId};

/// Encrypted TOTP secret for a user.
///
/// One per user. The `secret_ciphertext` includes the 16-byte Poly1305 tag.
/// The `nonce` is the 24-byte XChaCha20 nonce used during encryption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotpSecret {
    /// The user this secret belongs to.
    pub user_id: UserId,
    /// XChaCha20-Poly1305 ciphertext (plaintext + 16-byte auth tag).
    pub secret_ciphertext: Vec<u8>,
    /// 24-byte nonce for decryption.
    pub nonce: Vec<u8>,
    /// When the secret was created.
    pub created_at: Timestamp,
}

impl TotpSecret {
    /// Create a new TOTP secret record from encrypted components.
    #[must_use]
    pub fn new(user_id: UserId, secret_ciphertext: Vec<u8>, nonce: Vec<u8>) -> Self {
        Self {
            user_id,
            secret_ciphertext,
            nonce,
            created_at: Timestamp::now(),
        }
    }
}
