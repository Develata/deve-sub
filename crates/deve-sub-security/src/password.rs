//! Password hashing using argon2id.
//!
//! Uses the Argon2id algorithm with default parameters and a random salt.
//! The resulting PHC string is stored in the database. See
//! `docs/plan/00-engineering-constitution.md` §"Data and security".

use argon2::Argon2;
use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};

use crate::SecurityError;

/// Hash a plaintext password using argon2id with a random salt.
///
/// Returns a PHC-format string suitable for database storage.
///
/// # Errors
/// Returns [`SecurityError::PasswordHash`] if hashing fails (e.g. password
/// is empty or exceeds the maximum length).
pub fn hash_password(plain: &str) -> Result<String, SecurityError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| SecurityError::PasswordHash(e.to_string()))?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against a stored PHC-format hash.
///
/// Returns `Ok(true)` if the password matches, `Ok(false)` if it does not.
///
/// # Errors
/// Returns [`SecurityError::PasswordHash`] if the stored hash is malformed.
pub fn verify_password(plain: &str, phc_hash: &str) -> Result<bool, SecurityError> {
    let parsed =
        PasswordHash::new(phc_hash).map_err(|e| SecurityError::PasswordHash(e.to_string()))?;
    Ok(Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash_password("correct horse battery staple").expect("hash");
        assert!(verify_password("correct horse battery staple", &hash).expect("verify"));
    }

    #[test]
    fn verify_wrong_password() {
        let hash = hash_password("correct horse battery staple").expect("hash");
        assert!(!verify_password("wrong password", &hash).expect("verify"));
    }

    #[test]
    fn verify_malformed_hash() {
        assert!(verify_password("anything", "not-a-valid-phc-hash").is_err());
    }
}
