//! Authentication, crypto, and SSRF protection for Deve Sub.
//!
//! This crate is a placeholder in M1; real auth, session management,
//! and crypto utilities arrive in M2 (Auth and Users). See
//! `docs/plan/03-architecture.md` for the security layer's position.

#![cfg_attr(test, allow(clippy::expect_used))]

use thiserror::Error;

/// Errors produced by security operations.
#[derive(Debug, Error)]
pub enum SecurityError {
    /// A cryptographic operation failed.
    #[error("crypto error: {0}")]
    Crypto(String),

    /// A session token was invalid or expired.
    #[error("invalid session token")]
    InvalidSession,
}
