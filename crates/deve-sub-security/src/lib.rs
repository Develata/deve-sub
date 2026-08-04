//! Authentication, crypto, and SSRF protection for Deve Sub.
//!
//! This crate provides the cryptographic primitives used by the application
//! layer: argon2id password hashing, CSPRNG session token generation,
//! HMAC-SHA256 token hashing, and master key management. TOTP and recovery
//! code utilities arrive with M2 Slice 4 (2FA). See
//! `docs/plan/03-architecture.md` for the security layer's position.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod master_key;
pub mod password;
pub mod token;

pub use master_key::MasterKey;
pub use password::{hash_password, verify_password};
pub use token::{generate_session_token, hash_session_token};

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
