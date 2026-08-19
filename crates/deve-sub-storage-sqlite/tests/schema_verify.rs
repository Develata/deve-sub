#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Integration tests for `verify_schema` (DS-AUD-B06).
//!
//! The previous `verify_schema` only checked that `_sqlx_migrations` had a
//! row; it passed when the DB was behind, ahead, dirty, or had a checksum
//! mismatch. These tests pin each failure mode.

use deve_sub_storage_sqlite::{StorageError, embedded_schema_version, verify_schema};

/// Build the embedded Migrator (same source `verify_schema` uses).
fn embedded_migrator() -> sqlx::migrate::Migrator {
    sqlx::migrate!("../../migrations")
}

/// Connect to a temp SQLite DB (no auto-migrate).
async fn fresh_pool(db_path: &std::path::Path) -> sqlx::sqlite::SqlitePool {
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    sqlx::sqlite::SqlitePoolOptions::new()
        .connect(&url)
        .await
        .expect("connect")
}

/// Run all embedded migrations on the pool.
async fn migrate_all(pool: &sqlx::sqlite::SqlitePool) {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .expect("migrate");
}

/// Count of embedded migrations.
fn embedded_count() -> usize {
    embedded_migrator().migrations.len()
}

#[tokio::test]
async fn verify_ok_on_fully_migrated_db() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("test.db");
    let pool = fresh_pool(&db).await;
    migrate_all(&pool).await;

    verify_schema(&pool)
        .await
        .expect("fully-migrated DB must verify");

    pool.close().await;
}

#[tokio::test]
async fn verify_rejects_missing_migrations_table() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("test.db");
    let pool = fresh_pool(&db).await;

    let err = verify_schema(&pool)
        .await
        .expect_err("fresh DB must reject");
    assert!(matches!(err, StorageError::NotInitialized), "got {err:?}");

    pool.close().await;
}

#[tokio::test]
async fn verify_rejects_empty_migrations_table() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("test.db");
    let pool = fresh_pool(&db).await;
    // Create the table but leave it empty.
    sqlx::query(
        "CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY, description TEXT NOT NULL, \
         installed_on TIMESTAMP NOT NULL, success BOOLEAN NOT NULL, checksum BLOB NOT NULL, \
         execution_time BIGINT NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("create empty migrations table");

    let err = verify_schema(&pool)
        .await
        .expect_err("empty table must reject");
    assert!(matches!(err, StorageError::NotInitialized), "got {err:?}");

    pool.close().await;
}

#[tokio::test]
async fn verify_rejects_behind() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("test.db");
    let pool = fresh_pool(&db).await;
    migrate_all(&pool).await;

    // Delete the last N migration rows and reverse their schema changes is
    // hard; instead, delete the row for the highest migration only, which
    // makes the DB "behind" by one. The schema itself still has the
    // highest migration's tables (so queries would not fail), but
    // `_sqlx_migrations` no longer records it — which is exactly the
    // "behind" state from `verify_schema`'s perspective.
    let highest = embedded_schema_version();
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = ?")
        .bind(highest)
        .execute(&pool)
        .await
        .expect("delete highest migration row");

    let err = verify_schema(&pool)
        .await
        .expect_err("behind DB must reject");
    match err {
        StorageError::SchemaBehind { missing } => {
            assert_eq!(
                missing,
                vec![highest],
                "missing must be exactly the highest"
            );
        }
        other => panic!("expected SchemaBehind, got {other:?}"),
    }

    pool.close().await;
}

#[tokio::test]
async fn verify_rejects_ahead() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("test.db");
    let pool = fresh_pool(&db).await;
    migrate_all(&pool).await;

    // Insert a fake migration row with a version the binary does not embed.
    let fake_version = 9_999_i64;
    sqlx::query(
        "INSERT INTO _sqlx_migrations \
         (version, description, installed_on, success, checksum, execution_time) \
         VALUES (?, 'fake', '2026-01-01 00:00:00Z', 1, X'00', 0)",
    )
    .bind(fake_version)
    .execute(&pool)
    .await
    .expect("insert fake ahead migration");

    let err = verify_schema(&pool)
        .await
        .expect_err("ahead DB must reject");
    match err {
        StorageError::SchemaAhead { version } => {
            assert_eq!(version, fake_version, "ahead version must be the fake one");
        }
        other => panic!("expected SchemaAhead, got {other:?}"),
    }

    pool.close().await;
}

#[tokio::test]
async fn verify_rejects_dirty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("test.db");
    let pool = fresh_pool(&db).await;
    migrate_all(&pool).await;

    // Mark the first migration as dirty (success=0).
    let first_version = embedded_migrator()
        .migrations
        .iter()
        .map(|m| m.version)
        .min()
        .expect("at least one embedded migration");
    sqlx::query("UPDATE _sqlx_migrations SET success = 0 WHERE version = ?")
        .bind(first_version)
        .execute(&pool)
        .await
        .expect("mark dirty");

    let err = verify_schema(&pool)
        .await
        .expect_err("dirty DB must reject");
    match err {
        StorageError::SchemaDirty { version } => {
            assert_eq!(version, first_version, "dirty version must be the first");
        }
        other => panic!("expected SchemaDirty, got {other:?}"),
    }

    pool.close().await;
}

#[tokio::test]
async fn verify_rejects_checksum_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("test.db");
    let pool = fresh_pool(&db).await;
    migrate_all(&pool).await;

    // Corrupt the checksum of the first migration.
    let first_version = embedded_migrator()
        .migrations
        .iter()
        .map(|m| m.version)
        .min()
        .expect("at least one embedded migration");
    sqlx::query("UPDATE _sqlx_migrations SET checksum = X'DEADBEEF' WHERE version = ?")
        .bind(first_version)
        .execute(&pool)
        .await
        .expect("corrupt checksum");

    let err = verify_schema(&pool)
        .await
        .expect_err("mismatched DB must reject");
    match err {
        StorageError::SchemaChecksumMismatch { version } => {
            assert_eq!(version, first_version, "mismatch version must be the first");
        }
        other => panic!("expected SchemaChecksumMismatch, got {other:?}"),
    }

    pool.close().await;
}

#[tokio::test]
async fn embedded_count_is_nonzero() {
    // Sanity: the test suite assumes the binary embeds at least one
    // migration (otherwise behind/ahead/dirty tests are vacuous).
    assert!(embedded_count() > 0, "binary must embed migrations");
}
