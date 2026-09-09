#![allow(clippy::expect_used)]
//! BACKUP-002/003: online backups must describe and restore the archived snapshot.

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_deve-sub");

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backup002_concurrent_writes_preserve_manifest_and_restore() {
    let dir = tempfile::tempdir().expect("tempdir");
    let database = dir.path().join("live.sqlite");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("pool");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("migrations");

    let stop = Arc::new(AtomicBool::new(false));
    let writer_stop = Arc::clone(&stop);
    let writer_pool = pool.clone();
    let (ready, started) = tokio::sync::oneshot::channel();
    let writer = tokio::spawn(async move {
        let mut ready = Some(ready);
        let mut count = 0;
        while !writer_stop.load(Ordering::Relaxed) {
            sqlx::query("INSERT INTO audit_log (id, action) VALUES (?, 'test.backup')")
                .bind(format!("test-backup-{count}"))
                .execute(&writer_pool)
                .await
                .expect("audit write");
            count += 1;
            if let Some(ready) = ready.take() {
                let _ = ready.send(());
            }
        }
        count
    });
    started.await.expect("writer started");

    let archive = dir.path().join("backup.tar");
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(BIN)
            .arg("backup")
            .arg("--db-path")
            .arg(&database)
            .arg("--output")
            .arg(&archive)
            .kill_on_drop(true)
            .output(),
    )
    .await;
    // Stop and join before assertions so a failed CLI cannot leak test work.
    stop.store(true, Ordering::Relaxed);
    let writes = writer.await.expect("writer joined");
    pool.close().await;
    let output = result.expect("backup deadline").expect("backup process");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(writes > 1, "writer must remain active throughout backup");

    let mut tar = tar::Archive::new(std::fs::File::open(&archive).expect("archive"));
    let mut manifest = None;
    let snapshot = dir.path().join("snapshot.sqlite");
    for entry in tar.entries().expect("entries") {
        let mut entry = entry.expect("entry");
        match entry.path().expect("path").to_str() {
            Some("manifest.json") => {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).expect("manifest bytes");
                manifest =
                    Some(serde_json::from_slice::<serde_json::Value>(&bytes).expect("manifest"));
            }
            Some("database.sqlite") => {
                entry.unpack(&snapshot).expect("snapshot");
            }
            _ => {}
        }
    }
    let manifest = manifest.expect("manifest entry");
    let snapshot_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&snapshot)
                .read_only(true),
        )
        .await
        .expect("snapshot pool");
    let (snapshot_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_log")
        .fetch_one(&snapshot_pool)
        .await
        .expect("snapshot count");
    snapshot_pool.close().await;
    assert_eq!(
        manifest["row_counts"]["audit_log"].as_i64(),
        Some(snapshot_count)
    );

    let restored = dir.path().join("restored.sqlite");
    let output = tokio::process::Command::new(BIN)
        .arg("restore")
        .arg("--input")
        .arg(&archive)
        .arg("--db-path")
        .arg(&restored)
        .output()
        .await
        .expect("restore process");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
