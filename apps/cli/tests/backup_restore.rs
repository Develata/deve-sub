#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Integration tests for `deve-sub backup` and `deve-sub restore` (M11).
//!
//! BACKUP-001: backup creates a tar with manifest/database/config/metadata.
//! BACKUP-002: snapshot DB row counts match the manifest.
//! BACKUP-003: restore on a fresh instance passes integrity_check.
//! BACKUP-004: restore from an older-schema backup runs forward migrations.

use std::collections::BTreeMap;
use std::io::Read;
use std::process::Command;

use serde::Deserialize;

const BIN: &str = env!("CARGO_BIN_EXE_deve-sub");

#[derive(Debug, Deserialize)]
struct BackupManifest {
    version: u32,
    schema_version: i64,
    row_counts: BTreeMap<String, i64>,
}

/// Create a fully-migrated SQLite database at `db_path` and insert one row
/// into `users` so row counts are non-trivial.
async fn setup_db(db_path: &std::path::Path) {
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePool::connect(&url).await.expect("pool");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("migrations");
    sqlx::query("INSERT INTO users (id, username, password_hash, role, enabled, created_at) VALUES ('01HTEST000000000000000000A', 'admin', 'hash', 'admin', 1, '2025-01-01T00:00:00Z')")
        .execute(&pool)
        .await
        .expect("insert user");
    pool.close().await;
}

/// Create a database migrated only up to migration 13 (drops the
/// `traffic_daily_snapshots` table from migration 14 and removes its
/// migration row), simulating an older-schema backup source.
async fn setup_db_schema_13(db_path: &std::path::Path) {
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePool::connect(&url).await.expect("pool");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("migrations");
    sqlx::query("INSERT INTO users (id, username, password_hash, role, enabled, created_at) VALUES ('01HTEST000000000000000000A', 'admin', 'hash', 'admin', 1, '2025-01-01T00:00:00Z')")
        .execute(&pool)
        .await
        .expect("insert user");
    sqlx::query("DROP TABLE IF EXISTS traffic_daily_snapshots")
        .execute(&pool)
        .await
        .expect("drop table");
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 14")
        .execute(&pool)
        .await
        .expect("delete migration row");
    pool.close().await;
}

/// Read a file from a tar archive by name.
fn read_tar_entry(archive_path: &std::path::Path, name: &str) -> Option<Vec<u8>> {
    let file = std::fs::File::open(archive_path).expect("open archive");
    let mut archive = tar::Archive::new(file);
    for entry in archive.entries().expect("entries") {
        let mut entry = entry.expect("entry");
        let path = entry.path().expect("path").into_owned();
        if path.to_string_lossy() == name {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).expect("read");
            return Some(buf);
        }
    }
    None
}

/// List all entry names in a tar archive.
fn list_tar_entries(archive_path: &std::path::Path) -> Vec<String> {
    let file = std::fs::File::open(archive_path).expect("open archive");
    let mut archive = tar::Archive::new(file);
    let mut names = Vec::new();
    for entry in archive.entries().expect("entries") {
        let entry = entry.expect("entry");
        names.push(entry.path().expect("path").to_string_lossy().into_owned());
    }
    names
}

/// BACKUP-001: `deve-sub backup --output` creates a tar containing
/// manifest.json, database.sqlite, config.json, and metadata.json.
#[tokio::test(flavor = "multi_thread")]
async fn backup001_creates_complete_archive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    setup_db(&db_path).await;

    let backup_path = dir.path().join("backup.tar");
    let status = Command::new(BIN)
        .args([
            "backup",
            "--output",
            backup_path.to_str().unwrap(),
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .status()
        .expect("spawn");
    assert!(status.success(), "backup failed: {status:?}");

    let names = list_tar_entries(&backup_path);
    assert!(
        names.contains(&"manifest.json".to_owned()),
        "missing manifest.json: {names:?}"
    );
    assert!(
        names.contains(&"database.sqlite".to_owned()),
        "missing database.sqlite"
    );
    assert!(
        names.contains(&"config.json".to_owned()),
        "missing config.json"
    );
    assert!(
        names.contains(&"metadata.json".to_owned()),
        "missing metadata.json"
    );
}

/// BACKUP-002: snapshot database row counts match the manifest.
#[tokio::test(flavor = "multi_thread")]
async fn backup002_row_counts_match_manifest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    setup_db(&db_path).await;

    let backup_path = dir.path().join("backup.tar");
    let status = Command::new(BIN)
        .args([
            "backup",
            "--output",
            backup_path.to_str().unwrap(),
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .status()
        .expect("spawn");
    assert!(status.success(), "backup failed: {status:?}");

    let manifest_bytes = read_tar_entry(&backup_path, "manifest.json").expect("manifest");
    let manifest: BackupManifest = serde_json::from_slice(&manifest_bytes).expect("parse manifest");
    assert_eq!(manifest.version, 1);
    assert!(manifest.schema_version >= 14);
    assert_eq!(
        manifest.row_counts.get("users"),
        Some(&1),
        "one user row expected"
    );

    let snapshot_bytes = read_tar_entry(&backup_path, "database.sqlite").expect("snapshot");
    let snapshot_path = dir.path().join("snapshot.sqlite");
    std::fs::write(&snapshot_path, &snapshot_bytes).expect("write snapshot");

    let url = format!("sqlite://{}", snapshot_path.display());
    let pool = sqlx::sqlite::SqlitePool::connect(&url).await.expect("pool");
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count, manifest.row_counts["users"]);
    pool.close().await;
}

/// BACKUP-003: restore on a fresh instance restores the database and passes
/// integrity_check.
#[tokio::test(flavor = "multi_thread")]
async fn backup003_restore_passes_integrity_check() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src_db = dir.path().join("src.db");
    setup_db(&src_db).await;

    let backup_path = dir.path().join("backup.tar");
    let status = Command::new(BIN)
        .args([
            "backup",
            "--output",
            backup_path.to_str().unwrap(),
            "--db-path",
            src_db.to_str().unwrap(),
        ])
        .status()
        .expect("spawn");
    assert!(status.success(), "backup failed: {status:?}");

    let restore_db = dir.path().join("restored.db");
    let status = Command::new(BIN)
        .args([
            "restore",
            "--input",
            backup_path.to_str().unwrap(),
            "--db-path",
            restore_db.to_str().unwrap(),
        ])
        .status()
        .expect("spawn");
    assert!(status.success(), "restore failed: {status:?}");

    let url = format!("sqlite://{}", restore_db.display());
    let pool = sqlx::sqlite::SqlitePool::connect(&url).await.expect("pool");
    let (integrity,): (String,) = sqlx::query_as("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .expect("integrity");
    assert_eq!(integrity, "ok", "integrity check failed: {integrity}");
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count, 1, "restored DB should have the user row");
    pool.close().await;
}

/// BACKUP-004: restore from an older-schema backup runs forward migrations
/// and data is intact.
#[tokio::test(flavor = "multi_thread")]
async fn backup004_restore_runs_forward_migrations() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src_db = dir.path().join("src13.db");
    setup_db_schema_13(&src_db).await;

    let backup_path = dir.path().join("backup13.tar");
    let status = Command::new(BIN)
        .args([
            "backup",
            "--output",
            backup_path.to_str().unwrap(),
            "--db-path",
            src_db.to_str().unwrap(),
        ])
        .status()
        .expect("spawn");
    assert!(status.success(), "backup failed: {status:?}");

    let manifest_bytes = read_tar_entry(&backup_path, "manifest.json").expect("manifest");
    let manifest: BackupManifest = serde_json::from_slice(&manifest_bytes).expect("manifest");
    assert_eq!(manifest.schema_version, 13, "backup should be schema 13");

    let restore_db = dir.path().join("restored14.db");
    let status = Command::new(BIN)
        .args([
            "restore",
            "--input",
            backup_path.to_str().unwrap(),
            "--db-path",
            restore_db.to_str().unwrap(),
        ])
        .status()
        .expect("spawn");
    assert!(status.success(), "restore failed: {status:?}");

    let url = format!("sqlite://{}", restore_db.display());
    let pool = sqlx::sqlite::SqlitePool::connect(&url).await.expect("pool");

    let (max_ver,): (i64,) =
        sqlx::query_as("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .expect("max version");
    assert!(
        max_ver >= 14,
        "forward migration should bring schema to >= 14, got {max_ver}"
    );

    let table_exists: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='traffic_daily_snapshots'",
    )
    .fetch_one(&pool)
    .await
    .expect("table check");
    assert_eq!(
        table_exists.0, 1,
        "traffic_daily_snapshots table should exist after forward migration"
    );

    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count, 1, "user data should survive forward migration");
    pool.close().await;
}
