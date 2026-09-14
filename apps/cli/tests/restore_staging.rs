#![allow(clippy::expect_used)]
//! BACKUP-003: a prior restore's WAL must never become part of a new snapshot.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_deve-sub");

async fn run(command: &mut Command) -> std::process::Output {
    tokio::time::timeout(Duration::from_secs(30), command.kill_on_drop(true).output())
        .await
        .expect("CLI deadline")
        .expect("CLI output")
}

async fn fixture(directory: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let database = directory.join("source.sqlite");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", database.display()))
        .await
        .expect("pool");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("migrate");
    sqlx::query("INSERT INTO users (id, username, password_hash, role) VALUES ('01HTEST000000000000000000A', 'BACKUP_EXPECTED', 'fixture-hash', 'admin')")
        .execute(&pool).await.expect("seed");
    pool.close().await;
    let config = directory.join("config.json");
    std::fs::write(
        &config,
        serde_json::to_vec(&serde_json::json!({
            "database": {"path": database},
            "security": {"master_key_path": directory.join("unused.key")}
        }))
        .expect("config JSON"),
    )
    .expect("config");
    let archive = directory.join("backup.tar");
    let output = run(Command::new(BIN)
        .arg("backup")
        .arg("--config")
        .arg(&config)
        .arg("--output")
        .arg(&archive))
    .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (database, config, archive)
}

fn directory_entries(directory: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(directory)
        .expect("directory")
        .map(|entry| entry.expect("entry").path())
        .collect();
    paths.sort();
    paths
}

#[tokio::test]
async fn backup003_restore_ignores_previous_staging_wal() {
    let directory = tempfile::tempdir().expect("tempdir");
    let (database, config, archive) = fixture(directory.path()).await;
    let target = directory.path().join("restored.sqlite");
    let stale = directory.path().join(".restored.sqlite.restore-staging");
    std::fs::copy(database, &stale).expect("old staging");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&stale)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                .pragma("wal_autocheckpoint", "0"),
        )
        .await
        .expect("stale pool");
    sqlx::query("UPDATE users SET username = 'STALE_PREVIOUS_RESTORE'")
        .execute(&pool)
        .await
        .expect("stale write");
    // Capture the committed, uncheckpointed pair before closing, then replay
    // that exact filesystem state without depending on child-crash timing.
    let main_bytes = std::fs::read(&stale).expect("old main");
    let wal_path = directory
        .path()
        .join(".restored.sqlite.restore-staging-wal");
    let wal_bytes = std::fs::read(&wal_path).expect("old WAL");
    assert!(wal_bytes.len() > 32, "fixture must contain WAL frames");
    pool.close().await;
    std::fs::write(&stale, main_bytes).expect("restore crashed main");
    std::fs::write(&wal_path, &wal_bytes).expect("restore crashed WAL");

    let output = run(Command::new(BIN)
        .arg("restore")
        .arg("--config")
        .arg(config)
        .arg("--input")
        .arg(archive)
        .arg("--db-path")
        .arg(&target))
    .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let restored = sqlx::SqlitePool::connect(&format!("sqlite://{}", target.display()))
        .await
        .expect("restored pool");
    let (username,): (String,) = sqlx::query_as("SELECT username FROM users")
        .fetch_one(&restored)
        .await
        .expect("restored username");
    restored.close().await;
    assert_eq!(
        username, "BACKUP_EXPECTED",
        "restore must publish the archived content"
    );
    assert_eq!(
        std::fs::read(wal_path).expect("old WAL preserved"),
        wal_bytes
    );
    assert!(!directory_entries(directory.path()).iter().any(|path| {
        path.file_name()
            .expect("name")
            .to_string_lossy()
            .starts_with(".deve-sub-restore-")
    }));
}

#[tokio::test]
async fn backup003_failed_verification_cleans_own_staging() {
    let directory = tempfile::tempdir().expect("tempdir");
    let (database, config, _) = fixture(directory.path()).await;
    let target = directory.path().join("restored.sqlite");
    std::fs::copy(&database, &target).expect("existing target");
    let original = std::fs::read(&target).expect("target bytes");
    let archive = directory.path().join("bad-count.tar");
    let mut builder = tar::Builder::new(std::fs::File::create(&archive).expect("archive"));
    let manifest = br#"{"version":1,"schema_version":27,"created_at":"2025-01-01T00:00:00Z","row_counts":{"users":2}}"#;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    builder
        .append_data(&mut header, "manifest.json", &manifest[..])
        .expect("manifest");
    builder
        .append_path_with_name(&database, "database.sqlite")
        .expect("snapshot");
    builder.finish().expect("finish");
    let output = run(Command::new(BIN)
        .arg("restore")
        .arg("--config")
        .arg(config)
        .arg("--input")
        .arg(archive)
        .arg("--db-path")
        .arg(&target))
    .await;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("row count mismatches"));
    assert_eq!(std::fs::read(&target).expect("target preserved"), original);
    assert!(!directory_entries(directory.path()).iter().any(|path| {
        path.file_name()
            .expect("name")
            .to_string_lossy()
            .contains("restore-staging")
            || path
                .file_name()
                .expect("name")
                .to_string_lossy()
                .starts_with(".deve-sub-restore-")
    }));
}
