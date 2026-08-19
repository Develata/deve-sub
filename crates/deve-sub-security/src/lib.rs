//! Authentication, crypto, and SSRF protection for Deve Sub.
//!
//! This crate provides the cryptographic primitives used by the application
//! layer: argon2id password hashing, CSPRNG session token generation,
//! HMAC-SHA256 token hashing, TOTP (RFC 6238), recovery code generation,
//! XChaCha20-Poly1305 encryption, and master key management. See
//! `docs/plan/03-architecture.md` for the security layer's position.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod cipher;
pub mod envelope;
pub mod hmac;
pub mod master_key;
pub mod password;
pub mod recovery;
pub mod ssrf;
pub mod subkey;
pub mod token;
pub mod totp;

pub use cipher::{decrypt, decrypt_aad, decrypt_from_b64, encrypt, encrypt_aad, encrypt_to_b64};
pub use envelope::{is_envelope, mask_url, open, seal};
pub use hmac::{
    PURPOSE_CHALLENGE, PURPOSE_NODE_IDENTITY, PURPOSE_RECOVERY, PURPOSE_SESSION, hmac_digest,
    identity_fingerprint,
};
pub use master_key::MasterKey;
pub use password::{hash_password, hash_password_async, verify_password, verify_password_async};
pub use recovery::{
    generate_codes as generate_recovery_codes, normalize_code as normalize_recovery_code,
};
pub use ssrf::{SsrfError, SsrfGuard};
pub use subkey::derive_envelope_subkey;
pub use token::{generate_session_token, generate_short_code};
pub use totp::{
    DIGITS as TOTP_DIGITS, PERIOD as TOTP_PERIOD, base32_decode, base32_secret,
    generate_code as totp_generate_code, generate_code_string as totp_generate_code_string,
    generate_secret as totp_generate_secret, otpauth_uri as totp_otpauth_uri,
    verify_code as totp_verify_code,
};

use thiserror::Error;

/// Errors produced by security operations.
#[derive(Debug, Error)]
pub enum SecurityError {
    /// A cryptographic operation failed.
    #[error("crypto error: {0}")]
    Crypto(String),

    /// Password hashing or verification failed.
    #[error("password hash error: {0}")]
    PasswordHash(String),

    /// Master key loading or generation failed.
    #[error("master key error: {0}")]
    MasterKey(String),

    /// A session token was invalid or expired.
    #[error("invalid session token")]
    InvalidSession,
}
