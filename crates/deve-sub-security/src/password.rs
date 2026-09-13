//! Password hashing using argon2id.
//!
//! Uses the Argon2id algorithm with default parameters and a random salt.
//! The resulting PHC string is stored in the database. See
//! `docs/plan/00-engineering-constitution.md` §"Data and security".

use std::sync::{Arc, LazyLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use argon2::Argon2;
use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};

use crate::SecurityError;

// Bound Argon2 memory/CPU even when concurrent attempts pass the failure
// counters together. The blocking closure retains admission after cancellation.
static PASSWORD_WORK: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(8)));

async fn run_password_work<T: Send + 'static>(
    permit: OwnedSemaphorePermit,
    work: impl FnOnce() -> Result<T, SecurityError> + Send + 'static,
) -> Result<T, SecurityError> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|e| SecurityError::Crypto(format!("argon2 task join failed: {e}")))?
}

/// Hash a plaintext password using argon2id with a random salt.
///
/// Returns a PHC-format string suitable for database storage.
///
/// # Errors
/// Returns [`SecurityError::PasswordHash`] if hashing fails (e.g. password
/// exceeds the maximum length). Input validation (empty password, minimum
/// length) is enforced at the application layer, not here.
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

/// Async wrapper for [`hash_password`] that runs Argon2 on a blocking pool.
///
/// WHY: Argon2 is CPU-intensive (~20-50ms with default params) and blocks the
/// calling thread. Calling it directly in an async function parks the tokio
/// worker for that duration, starving other futures on the same worker. This
/// wrapper offloads the hashing to `tokio::task::spawn_blocking` so the worker
/// is free to poll other tasks.
///
/// # Errors
/// Returns [`SecurityError::PasswordHash`] if hashing fails (propagated from
/// [`hash_password`]).
pub async fn hash_password_async(plain: String) -> Result<String, SecurityError> {
    let permit = PASSWORD_WORK
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| SecurityError::PasswordWorkBusy)?;
    run_password_work(permit, move || hash_password(&plain)).await
}

/// Async wrapper for [`verify_password`] that runs Argon2 on a blocking pool.
///
/// WHY: same as [`hash_password_async`] — `verify_password` is CPU-intensive
/// and would block the tokio worker if called directly from an async function.
///
/// # Errors
/// Returns [`SecurityError::PasswordHash`] if the stored hash is malformed
/// (propagated from [`verify_password`]). Returns
/// [`SecurityError::Crypto`] if the blocking task panics or is cancelled.
pub async fn verify_password_async(plain: String, phc_hash: String) -> Result<bool, SecurityError> {
    let permit = PASSWORD_WORK
        .clone()
        .try_acquire_owned()
        .map_err(|_| SecurityError::PasswordWorkBusy)?;
    run_password_work(permit, move || verify_password(&plain, &phc_hash)).await
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

#[cfg(test)]
mod admission_tests {
    use super::*;

    #[tokio::test]
    async fn password_verification_rejects_saturated_budget() {
        let _all_workers = PASSWORD_WORK
            .clone()
            .acquire_many_owned(8)
            .await
            .expect("budget");
        assert!(matches!(
            verify_password_async("fixture-password".into(), "invalid-hash".into()).await,
            Err(SecurityError::PasswordWorkBusy)
        ));
    }

    #[tokio::test]
    async fn cancelled_caller_keeps_worker_permit_until_blocking_job_finishes() {
        let budget = Arc::new(Semaphore::new(1));
        let permit = budget.clone().try_acquire_owned().expect("permit");
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(run_password_work(permit, move || {
            let _ = entered_tx.send(());
            finish_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("bounded work");
            Ok(())
        }));
        entered_rx.await.expect("entered");
        task.abort();
        assert!(task.await.expect_err("cancelled").is_cancelled());
        assert!(budget.clone().try_acquire_owned().is_err());
        finish_tx.send(()).expect("finish");
        let _next = tokio::time::timeout(std::time::Duration::from_secs(5), budget.acquire())
            .await
            .expect("released")
            .expect("permit");
    }
}
