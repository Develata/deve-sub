#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Integration tests for `ensure_key_binding` (DS-AUD-B07).
//!
//! The binding pins a fresh DB to the first master key that opens it, and
//! rejects a subsequent key that does not match — preventing a management
//! command from silently generating a NEW key on a host with an existing
//! DB whose key file was lost/misconfigured.

use deve_sub_storage_sqlite::{StorageError, ensure_key_binding};

async fn fresh_pool(db_path: &std::path::Path) -> sqlx::sqlite::SqlitePool {
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect(&url)
        .await
        .expect("connect");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("migrate");
    pool
}

#[tokio::test]
async fn binds_key_on_first_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = fresh_pool(&dir.path().join("test.db")).await;

    ensure_key_binding(&pool, "fp-A")
        .await
        .expect("first open binds");

    let (bound,): (String,) =
        sqlx::query_as("SELECT current_key_fingerprint FROM key_metadata WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("read bound fingerprint");
    assert_eq!(bound, "fp-A");

    pool.close().await;
}

#[tokio::test]
async fn accepts_matching_key_on_subsequent_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = fresh_pool(&dir.path().join("test.db")).await;

    ensure_key_binding(&pool, "fp-A").await.expect("first open");
    ensure_key_binding(&pool, "fp-A")
        .await
        .expect("same key must be accepted");

    pool.close().await;
}

#[tokio::test]
async fn rejects_mismatched_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = fresh_pool(&dir.path().join("test.db")).await;

    ensure_key_binding(&pool, "fp-A").await.expect("first open");

    let err = ensure_key_binding(&pool, "fp-B")
        .await
        .expect_err("different key must reject");
    match err {
        StorageError::KeyFingerprintMismatch { expected, actual } => {
            assert_eq!(expected, "fp-A");
            assert_eq!(actual, "fp-B");
        }
        other => panic!("expected KeyFingerprintMismatch, got {other:?}"),
    }

    pool.close().await;
}

#[tokio::test]
async fn rejects_mismatched_key_after_matching_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = fresh_pool(&dir.path().join("test.db")).await;

    ensure_key_binding(&pool, "fp-A").await.expect("first open");
    ensure_key_binding(&pool, "fp-A")
        .await
        .expect("matching key accepted");
    ensure_key_binding(&pool, "fp-C")
        .await
        .expect_err("third key must reject");

    pool.close().await;
}

#[tokio::test]
async fn second_key_after_mismatch_still_rejects_original_bound() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = fresh_pool(&dir.path().join("test.db")).await;

    // WHY: a failed bind attempt must not alter the bound key. The DB stays
    // bound to fp-A; a later attempt with fp-D must still report fp-A as the
    // expected fingerprint, not fp-D or a corrupted state.
    ensure_key_binding(&pool, "fp-A").await.expect("first open");
    let _ = ensure_key_binding(&pool, "fp-D").await;
    let err = ensure_key_binding(&pool, "fp-E")
        .await
        .expect_err("fp-E must reject");
    match err {
        StorageError::KeyFingerprintMismatch { expected, actual } => {
            assert_eq!(expected, "fp-A");
            assert_eq!(actual, "fp-E");
        }
        other => panic!("expected KeyFingerprintMismatch with fp-A expected, got {other:?}"),
    }

    pool.close().await;
}
