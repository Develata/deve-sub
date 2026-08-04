//! Recovery code entity — single-use 2FA fallback codes.
//!
//! Recovery codes are high-entropy random strings stored as HMAC-SHA256
//! hashes. Each code can be used exactly once. When a user regenerates
//! recovery codes, all existing codes are deleted and a new batch is stored.

use deve_sub_kernel::{RecoveryCodeId, Timestamp, UserId};

/// A single-use recovery code for 2FA fallback.
///
/// The `code_hash` is the HMAC-SHA256 digest of the normalized code string
/// (uppercase, no separators). The raw code is never persisted; it is shown
/// to the user once at generation time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryCode {
    /// Unique identifier (ULID).
    pub id: RecoveryCodeId,
    /// The user this code belongs to.
    pub user_id: UserId,
    /// HMAC-SHA256 digest of the normalized code.
    pub code_hash: String,
    /// Whether this code has been used.
    pub used: bool,
    /// When the code was created.
    pub created_at: Timestamp,
}

impl RecoveryCode {
    /// Create a new unused recovery code.
    #[must_use]
    pub fn new(user_id: UserId, code_hash: String) -> Self {
        Self {
            id: RecoveryCodeId::new(),
            user_id,
            code_hash,
            used: false,
            created_at: Timestamp::now(),
        }
    }
}
