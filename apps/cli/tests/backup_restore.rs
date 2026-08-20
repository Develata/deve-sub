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
    #[serde(default)]
    master_key_fingerprint: Option<String>,
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

/// Create a database migrated only up to migration 13 (reverses migrations
/// 0014 and 0015, then removes their migration rows), simulating an
/// older-schema backup source.
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

    // Reverse migration 0014: drop the traffic_daily_snapshots table.
    sqlx::query("DROP TABLE IF EXISTS traffic_daily_snapshots")
        .execute(&pool)
        .await
        .expect("drop table");

    // Reverse migration 0015: drop the _encrypted columns and restore the
    // plaintext columns that 0015 dropped, so the schema matches pre-0015
    // state and forward-migrating through 0015 succeeds on restore.
    for stmt in [
        "ALTER TABLE sources DROP COLUMN url_encrypted",
        "ALTER TABLE sources DROP COLUMN headers_encrypted",
        "ALTER TABLE sources ADD COLUMN url TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE sources ADD COLUMN headers_encrypted TEXT",
        "ALTER TABLE source_items DROP COLUMN raw_uri_encrypted",
        "ALTER TABLE source_items ADD COLUMN raw_uri TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE node_source_bindings DROP COLUMN raw_uri_encrypted",
        "ALTER TABLE node_source_bindings ADD COLUMN raw_uri TEXT",
        "ALTER TABLE nodes DROP COLUMN protocol_config_json_encrypted",
        "ALTER TABLE nodes DROP COLUMN authentication_json_encrypted",
        "ALTER TABLE nodes DROP COLUMN tls_json_encrypted",
        "ALTER TABLE nodes DROP COLUMN transport_json_encrypted",
        "ALTER TABLE nodes DROP COLUMN obfuscation_json_encrypted",
        "ALTER TABLE nodes DROP COLUMN extras_json_encrypted",
        "ALTER TABLE nodes ADD COLUMN protocol_config_json TEXT NOT NULL DEFAULT '{}'",
        "ALTER TABLE nodes ADD COLUMN authentication_json TEXT NOT NULL DEFAULT '{}'",
        "ALTER TABLE nodes ADD COLUMN tls_json TEXT",
        "ALTER TABLE nodes ADD COLUMN transport_json TEXT",
        "ALTER TABLE nodes ADD COLUMN obfuscation_json TEXT",
        "ALTER TABLE nodes ADD COLUMN extras_json TEXT NOT NULL DEFAULT '{}'",
    ] {
        sqlx::query(stmt)
            .execute(&pool)
            .await
            .expect("reverse 0015");
    }

    // Reverse migration 0016: drop the `mode` column from generation_cache.
    sqlx::query("ALTER TABLE generation_cache DROP COLUMN mode")
        .execute(&pool)
        .await
        .expect("reverse 0016");

    // Reverse migration 0017: drop identity_fingerprint column and restore
    // the old (protocol_kind, host, port) dedup unique index.
    sqlx::query("DROP INDEX IF EXISTS idx_nodes_dedup")
        .execute(&pool)
        .await
        .expect("drop idx_nodes_dedup for 0017 reversal");
    sqlx::query("ALTER TABLE nodes DROP COLUMN identity_fingerprint")
        .execute(&pool)
        .await
        .expect("reverse 0017");
    sqlx::query(
        "CREATE UNIQUE INDEX idx_nodes_dedup \
         ON nodes(protocol_kind, host, port) WHERE missing_from_source = 0",
    )
    .execute(&pool)
    .await
    .expect("recreate old idx_nodes_dedup");

    // Reverse migration 0018: drop the key_metadata table so forward-
    // migrating through 0018 succeeds on restore.
    sqlx::query("DROP TABLE IF EXISTS key_metadata")
        .execute(&pool)
        .await
        .expect("reverse 0018");

    // Reverse migration 0019: drop source_refresh_jobs table and the
    // snapshots (source_id, version) unique index so forward-migrating
    // through 0019 succeeds on restore.
    sqlx::query("DROP INDEX IF EXISTS idx_snapshots_source_version_unique")
        .execute(&pool)
        .await
        .expect("reverse 0019 index");
    sqlx::query("DROP TABLE IF EXISTS source_refresh_jobs")
        .execute(&pool)
        .await
        .expect("reverse 0019 table");

    // Reverse migration 0020: drop the UNIQUE indexes so forward-migrating
    // through 0020 (which does DROP INDEX IF EXISTS then CREATE UNIQUE INDEX)
    // succeeds on restore.
    sqlx::query("DROP INDEX IF EXISTS idx_template_versions_template")
        .execute(&pool)
        .await
        .expect("drop 0020 template_versions unique");
    sqlx::query("DROP INDEX IF EXISTS idx_subscription_tokens_subscription")
        .execute(&pool)
        .await
        .expect("drop 0020 subscription_tokens unique");
    sqlx::query("DROP INDEX IF EXISTS idx_subscription_short_codes_subscription")
        .execute(&pool)
        .await
        .expect("drop 0020 subscription_short_codes unique");

    sqlx::query("DELETE FROM _sqlx_migrations WHERE version >= 14")
        .execute(&pool)
        .await
        .expect("delete migration rows");
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

/// BACKUP-005 (DS-AUD-031): tar archive entries use mode 0o600, not 0o644.
/// The archive contains the full production DB — credentials, tokens, TOTP
/// secrets — so it must not be world-readable.
#[tokio::test(flavor = "multi_thread")]
async fn backup005_archive_entries_use_restricted_permissions() {
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

    let file = std::fs::File::open(&backup_path).expect("open archive");
    let mut archive = tar::Archive::new(file);
    for entry in archive.entries().expect("entries") {
        let entry = entry.expect("entry");
        let header = entry.header();
        let mode = header.mode().unwrap_or(0);
        let name = entry.path().expect("path").to_string_lossy().into_owned();
        assert_eq!(
            mode & 0o777,
            0o600,
            "archive entry '{name}' has mode {mode:o}, expected 0o600"
        );
    }
}

/// BACKUP-006 (DS-AUD-034): when --key-path is provided, the manifest records
/// the SHA-256 fingerprint of the master key.
#[tokio::test(flavor = "multi_thread")]
async fn backup006_manifest_records_key_fingerprint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    setup_db(&db_path).await;

    let key_path = dir.path().join("master.key");
    let key_bytes = [0xABu8; 32];
    std::fs::write(&key_path, key_bytes).expect("write key");

    let backup_path = dir.path().join("backup.tar");
    let status = Command::new(BIN)
        .args([
            "backup",
            "--output",
            backup_path.to_str().unwrap(),
            "--db-path",
            db_path.to_str().unwrap(),
            "--key-path",
            key_path.to_str().unwrap(),
        ])
        .status()
        .expect("spawn");
    assert!(status.success(), "backup failed: {status:?}");

    let manifest_bytes = read_tar_entry(&backup_path, "manifest.json").expect("manifest");
    let manifest: BackupManifest = serde_json::from_slice(&manifest_bytes).expect("parse manifest");
    let fp = manifest
        .master_key_fingerprint
        .expect("manifest must record fingerprint when --key-path is set");
    assert_eq!(fp.len(), 64, "SHA-256 hex = 64 chars");
    assert!(
        fp.chars().all(|c| c.is_ascii_hexdigit()),
        "fingerprint must be hex"
    );

    // WHY use `MasterKey::fingerprint` as the oracle rather than
    // re-deriving the HMAC: this test confirms the manifest *records* the
    // fingerprint, not that the HMAC construction is correct (that belongs
    // in the security crate's own tests).
    let expected_fp = deve_sub_security::MasterKey::from_bytes(&key_bytes)
        .fingerprint()
        .expect("fingerprint");
    assert_eq!(
        fp, expected_fp,
        "fingerprint must match MasterKey::fingerprint (HMAC-SHA256 keyed by master key)"
    );
}

/// BACKUP-007 (DS-AUD-034): restore with the correct key succeeds and
/// reports fingerprint verification.
#[tokio::test(flavor = "multi_thread")]
async fn backup007_restore_with_matching_key_succeeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src_db = dir.path().join("src.db");
    setup_db(&src_db).await;

    let key_path = dir.path().join("master.key");
    std::fs::write(&key_path, [0xABu8; 32]).expect("write key");

    let backup_path = dir.path().join("backup.tar");
    let status = Command::new(BIN)
        .args([
            "backup",
            "--output",
            backup_path.to_str().unwrap(),
            "--db-path",
            src_db.to_str().unwrap(),
            "--key-path",
            key_path.to_str().unwrap(),
        ])
        .status()
        .expect("spawn");
    assert!(status.success(), "backup failed: {status:?}");

    let restore_db = dir.path().join("restored.db");
    let output = Command::new(BIN)
        .args([
            "restore",
            "--input",
            backup_path.to_str().unwrap(),
            "--db-path",
            restore_db.to_str().unwrap(),
            "--key-path",
            key_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn");
    assert!(
        output.status.success(),
        "restore failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("key fingerprint: verified"),
        "restore output should confirm fingerprint verification: {stdout}"
    );
}

/// BACKUP-008 (DS-AUD-034): restore with a mismatched key is refused.
#[tokio::test(flavor = "multi_thread")]
async fn backup008_restore_with_mismatched_key_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src_db = dir.path().join("src.db");
    setup_db(&src_db).await;

    let backup_key_path = dir.path().join("backup.key");
    std::fs::write(&backup_key_path, [0xABu8; 32]).expect("write backup key");

    let backup_path = dir.path().join("backup.tar");
    let status = Command::new(BIN)
        .args([
            "backup",
            "--output",
            backup_path.to_str().unwrap(),
            "--db-path",
            src_db.to_str().unwrap(),
            "--key-path",
            backup_key_path.to_str().unwrap(),
        ])
        .status()
        .expect("spawn");
    assert!(status.success(), "backup failed: {status:?}");

    let wrong_key_path = dir.path().join("wrong.key");
    std::fs::write(&wrong_key_path, [0xCDu8; 32]).expect("write wrong key");

    let restore_db = dir.path().join("restored.db");
    let output = Command::new(BIN)
        .args([
            "restore",
            "--input",
            backup_path.to_str().unwrap(),
            "--db-path",
            restore_db.to_str().unwrap(),
            "--key-path",
            wrong_key_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn");
    assert!(
        !output.status.success(),
        "restore with mismatched key must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("fingerprint mismatch"),
        "error must mention fingerprint mismatch: {stderr}"
    );
    assert!(
        !restore_db.exists(),
        "restore DB must not be created on key mismatch"
    );
}

/// BACKUP-009 (DS-AUD-032): a failed restore (corrupt snapshot) leaves the
/// existing production DB intact. The restore writes to a staging file,
/// verifies, and only renames on success.
#[tokio::test(flavor = "multi_thread")]
async fn backup009_failed_restore_preserves_existing_db() {
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

    // Corrupt the archive by truncating the database.sqlite entry.
    let corrupt_path = dir.path().join("corrupt.tar");
    {
        let file = std::fs::File::open(&backup_path).expect("open backup");
        let mut archive = tar::Archive::new(file);
        let entries: Vec<tar::Entry<_>> = archive
            .entries()
            .expect("entries")
            .collect::<Result<_, _>>()
            .expect("collect entries");
        let out_file = std::fs::File::create(&corrupt_path).expect("create corrupt");
        let mut builder = tar::Builder::new(out_file);
        for mut entry in entries {
            let path = entry.path().expect("path").into_owned();
            let name = path.to_string_lossy().into_owned();
            let mut header = entry.header().clone();
            if name == "database.sqlite" {
                let garbage = b"not a database";
                header.set_size(garbage.len() as u64);
                header.set_cksum();
                builder
                    .append(&header, std::io::Cursor::new(garbage))
                    .expect("append");
            } else {
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf).expect("read");
                builder
                    .append(&header, std::io::Cursor::new(buf))
                    .expect("append");
            }
        }
        builder.finish().expect("finish");
    }

    let prod_db = dir.path().join("prod.db");
    setup_db(&prod_db).await;
    let prod_url = format!("sqlite://{}", prod_db.display());
    let prod_pool = sqlx::sqlite::SqlitePool::connect(&prod_url)
        .await
        .expect("pool");
    let (prod_count_before,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&prod_pool)
        .await
        .expect("count");
    assert_eq!(prod_count_before, 1);
    prod_pool.close().await;

    let output = Command::new(BIN)
        .args([
            "restore",
            "--input",
            corrupt_path.to_str().unwrap(),
            "--db-path",
            prod_db.to_str().unwrap(),
        ])
        .output()
        .expect("spawn");
    assert!(
        !output.status.success(),
        "restore of corrupt archive must fail"
    );

    let verify_pool = sqlx::sqlite::SqlitePool::connect(&prod_url)
        .await
        .expect("pool");
    let (prod_count_after,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&verify_pool)
        .await
        .expect("count");
    assert_eq!(
        prod_count_after, 1,
        "production DB must be preserved after failed restore"
    );
    verify_pool.close().await;
}
