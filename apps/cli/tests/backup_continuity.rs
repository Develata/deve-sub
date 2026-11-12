#![allow(clippy::expect_used)]
//! BACKUP-001/003/004: archived bindings and migration history guard the swap.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use tokio::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_deve-sub");

fn fingerprint(byte: u8) -> String {
    deve_sub_security::MasterKey::from_bytes(&[byte; 32])
        .fingerprint()
        .expect("fingerprint")
}

async fn database(path: &Path, version: i64, bound: Option<&str>, username: &str) {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .expect("pool");
    let mut migrations = sqlx::migrate!("../../migrations");
    migrations.migrations = migrations
        .migrations
        .iter()
        .filter(|migration| migration.version <= version)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    migrations.run(&pool).await.expect("migrate fixture");
    sqlx::query("INSERT INTO users (id, username, password_hash, role) VALUES ('01HTEST000000000000000000A', ?, 'synthetic-hash', 'admin')")
        .bind(username).execute(&pool).await.expect("user");
    if let Some(binding) = bound {
        sqlx::query("INSERT INTO key_metadata (id, current_key_fingerprint) VALUES (1, ?)")
            .bind(binding)
            .execute(&pool)
            .await
            .expect("binding");
    }
    pool.close().await;
}

fn config(directory: &Path) -> PathBuf {
    let path = directory.join("config.json");
    std::fs::write(
        &path,
        json!({"security": {"master_key_path": directory.join("absent-default.key")}}).to_string(),
    )
    .expect("config");
    path
}

fn command(config: &Path, operation: &str, db: &Path, archive: &Path) -> Command {
    let mut command = Command::new(BIN);
    command
        .env_remove("DEVE_SUB_KEY_PATH")
        .env_remove("DEVE_SUB_CONFIG")
        .arg(operation)
        .arg("--config")
        .arg(config)
        .arg("--db-path")
        .arg(db)
        .arg(if operation == "backup" {
            "--output"
        } else {
            "--input"
        })
        .arg(archive);
    command
}

async fn run(command: &mut Command) -> std::process::Output {
    tokio::time::timeout(Duration::from_secs(30), command.kill_on_drop(true).output())
        .await
        .expect("CLI deadline")
        .expect("CLI output")
}

fn archive(path: &Path, db: &Path, version: i64, binding: Option<&str>) {
    let manifest = json!({"version":1, "schema_version":version,
        "created_at":"2025-01-01T00:00:00Z", "row_counts":{"users":1},
        "master_key_fingerprint":binding})
    .to_string();
    let mut builder = tar::Builder::new(std::fs::File::create(path).expect("archive"));
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    builder
        .append_data(&mut header, "manifest.json", manifest.as_bytes())
        .expect("manifest");
    builder
        .append_path_with_name(db, "database.sqlite")
        .expect("snapshot");
    builder.finish().expect("finish");
}

fn rejected(output: &std::process::Output, reason: &str, target: &Path, before: &[u8]) {
    assert!(!output.status.success(), "unexpected success");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(reason), "expected {reason}: {stderr}");
    assert_eq!(std::fs::read(target).expect("preserved target"), before);
    assert!(
        !target
            .parent()
            .expect("parent")
            .read_dir()
            .expect("entries")
            .any(|entry| entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".deve-sub-restore-"))
    );
}

fn current() -> i64 {
    deve_sub_storage_sqlite::embedded_schema_version()
}

#[tokio::test]
async fn backup_bound_snapshot_requires_matching_key_and_preserves_archive() {
    let dir = tempfile::tempdir().expect("directory");
    let config = config(dir.path());
    let db = dir.path().join("source.sqlite");
    let output = dir.path().join("backup.tar");
    database(&db, current(), Some(&fingerprint(0x11)), "SOURCE").await;
    std::fs::write(&output, b"previous verified backup").expect("previous archive");
    let previous = std::fs::read(&output).expect("previous bytes");
    let wrong = dir.path().join("wrong.key");
    std::fs::write(&wrong, [0x22; 32]).expect("key");
    for via_environment in [false, true] {
        let mut cmd = command(&config, "backup", &db, &output);
        if via_environment {
            cmd.env("DEVE_SUB_KEY_PATH", &wrong);
        } else {
            cmd.arg("--key-path").arg(&wrong);
        }
        rejected(
            &run(&mut cmd).await,
            "fingerprint mismatch",
            &output,
            &previous,
        );
    }
    rejected(
        &run(&mut command(&config, "backup", &db, &output)).await,
        "requires the original key",
        &output,
        &previous,
    );
    let correct = dir.path().join("correct.key");
    std::fs::write(&correct, [0x11; 32]).expect("correct key");
    let result = run(command(&config, "backup", &db, &output)
        .arg("--key-path")
        .arg(&correct))
    .await;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[tokio::test]
async fn restore_checks_database_binding_even_when_manifest_matches_wrong_key() {
    let dir = tempfile::tempdir().expect("directory");
    let config = config(dir.path());
    let db = dir.path().join("source.sqlite");
    database(&db, current(), Some(&fingerprint(0x11)), "SOURCE").await;
    let backup = dir.path().join("backup.tar");
    for (index, (manifest, loaded, reason)) in [
        (
            Some(fingerprint(0x22)),
            Some(0x22),
            Some("fingerprint mismatch with database binding"),
        ),
        (None, None, Some("requires the original key")),
        (None, Some(0x11), None),
        (Some(fingerprint(0x11)), Some(0x11), None),
    ]
    .into_iter()
    .enumerate()
    {
        archive(&backup, &db, current(), manifest.as_deref());
        let target = dir.path().join(format!("target-{index}.sqlite"));
        database(&target, current(), None, "UNIQUE_TARGET_MARKER").await;
        let before = std::fs::read(&target).expect("target bytes");
        let mut cmd = command(&config, "restore", &target, &backup);
        if let Some(byte) = loaded {
            let key = dir.path().join("loaded.key");
            std::fs::write(&key, [byte; 32]).expect("loaded key");
            cmd.arg("--key-path").arg(key);
        }
        let output = run(&mut cmd).await;
        if let Some(reason) = reason {
            rejected(&output, reason, &target, &before);
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", target.display()))
                .await
                .expect("restored pool");
            let actual: String =
                sqlx::query_scalar("SELECT current_key_fingerprint FROM key_metadata WHERE id=1")
                    .fetch_one(&pool)
                    .await
                    .expect("binding");
            assert_eq!(actual, fingerprint(0x11));
            pool.close().await;
        }
    }
}

#[tokio::test]
async fn restore_explicit_invalid_key_cannot_disappear_with_legacy_manifest() {
    let dir = tempfile::tempdir().expect("directory");
    let config = config(dir.path());
    let source = dir.path().join("source.sqlite");
    database(&source, current(), None, "SOURCE").await;
    let backup = dir.path().join("backup.tar");
    archive(&backup, &source, current(), None);
    let target = dir.path().join("target.sqlite");
    database(&target, current(), None, "TARGET").await;
    let before = std::fs::read(&target).expect("target bytes");
    for via_environment in [false, true] {
        for kind in ["missing", "directory", "short"] {
            let key = dir.path().join(kind);
            if kind == "directory" && !key.exists() {
                std::fs::create_dir(&key).expect("key directory");
            }
            if kind == "short" {
                std::fs::write(&key, b"short").expect("short key");
            }
            let mut cmd = command(&config, "restore", &target, &backup);
            if via_environment {
                cmd.env("DEVE_SUB_KEY_PATH", key);
            } else {
                cmd.arg("--key-path").arg(key);
            }
            rejected(
                &run(&mut cmd).await,
                "explicitly requested",
                &target,
                &before,
            );
        }
    }
}

#[tokio::test]
async fn restore_rejects_tampered_migration_history_before_swap() {
    let dir = tempfile::tempdir().expect("directory");
    let config = config(dir.path());
    let target = dir.path().join("target.sqlite");
    database(&target, current(), None, "TARGET").await;
    let before = std::fs::read(&target).expect("target bytes");
    for (index, (mutation, version, reason)) in [
        (
            "UPDATE _sqlx_migrations SET checksum = X'00' WHERE version=1",
            current(),
            "checksum mismatch",
        ),
        (
            "UPDATE _sqlx_migrations SET success = 0 WHERE version=1",
            current(),
            "dirty",
        ),
        (
            "DELETE FROM _sqlx_migrations WHERE version=7",
            current(),
            "migration history is invalid",
        ),
        ("SELECT 1", current() - 1, "schema version mismatch"),
        (
            "DROP TABLE key_metadata",
            current(),
            "key_metadata table is missing",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let source = dir.path().join(format!("source-{index}.sqlite"));
        database(&source, current(), None, "SOURCE").await;
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", source.display()))
            .await
            .expect("pool");
        sqlx::query(mutation)
            .execute(&pool)
            .await
            .expect("tamper fixture");
        pool.close().await;
        let backup = dir.path().join("tampered.tar");
        archive(&backup, &source, version, None);
        rejected(
            &run(&mut command(&config, "restore", &target, &backup)).await,
            reason,
            &target,
            &before,
        );
    }
}

#[tokio::test]
async fn legacy_user_only_archive_upgrades_without_creating_key_binding() {
    let dir = tempfile::tempdir().expect("directory");
    let config = config(dir.path());
    let db = dir.path().join("old.sqlite");
    database(&db, 13, None, "LEGACY_USER_ONLY").await;
    let backup = dir.path().join("old.tar");
    let output = run(&mut command(&config, "backup", &db, &backup)).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let target = dir.path().join("restored.sqlite");
    let output = run(&mut command(&config, "restore", &target, &backup)).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", target.display()))
        .await
        .expect("pool");
    deve_sub_storage_sqlite::verify_schema(&pool)
        .await
        .expect("complete current schema");
    let name: String = sqlx::query_scalar("SELECT username FROM users")
        .fetch_one(&pool)
        .await
        .expect("user");
    assert_eq!(name, "LEGACY_USER_ONLY");
    let bindings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM key_metadata")
        .fetch_one(&pool)
        .await
        .expect("metadata");
    assert_eq!(bindings, 0);
    assert!(!dir.path().join("absent-default.key").exists());
    pool.close().await;
}
