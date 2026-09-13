#![allow(clippy::expect_used, clippy::unwrap_used)]

//! AUTH-001: real CLI processes share the atomic administrator setup path.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Output;
use std::time::Duration;

use tokio::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_deve-sub");
const PASSWORD: &str = "fixture-$bootstrap-password!";
const PASSWORD_ENV: &str = "DEVE_SUB_TEST_BOOTSTRAP_PASSWORD";

async fn init(db: &Path, username: &str, password: Option<&OsStr>, if_needed: bool) -> Output {
    let mut command = Command::new(BIN);
    command
        .args([
            "user",
            "init-admin",
            "--username",
            username,
            "--password-env",
            PASSWORD_ENV,
        ])
        .arg("--db-path")
        .arg(db)
        .env_remove(PASSWORD_ENV)
        .kill_on_drop(true);
    if if_needed {
        command.arg("--if-needed");
    }
    if let Some(password) = password {
        command.env(PASSWORD_ENV, password);
    }
    let output = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .expect("bounded initialization")
        .expect("spawn CLI");
    for bytes in [&output.stdout, &output.stderr] {
        assert!(
            !String::from_utf8_lossy(bytes).contains(PASSWORD),
            "password leaked"
        );
    }
    output
}

async fn pool(db: &Path) -> sqlx::SqlitePool {
    sqlx::SqlitePool::connect(&format!("sqlite://{}", db.display()))
        .await
        .expect("open DB")
}

async fn users(db: &Path) -> Vec<(String, String, String, i64)> {
    let pool = pool(db).await;
    let rows = sqlx::query_as("SELECT username, password_hash, role, enabled FROM users")
        .fetch_all(&pool)
        .await
        .expect("read users");
    pool.close().await;
    rows
}

#[tokio::test]
async fn env_admin_creates_once_and_preserves_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("bootstrap.db");
    assert!(
        init(&db, "fixture-admin", Some(OsStr::new(PASSWORD)), true)
            .await
            .status
            .success()
    );
    let before = users(&db).await;
    assert_eq!(before.len(), 1);
    assert_eq!(
        (&*before[0].0, &*before[0].2, before[0].3),
        ("fixture-admin", "admin", 1)
    );
    assert!(before[0].1.starts_with("$argon2id$"));
    assert!(deve_sub_security::verify_password(PASSWORD, &before[0].1).unwrap());

    assert!(
        init(
            &db,
            "replacement-admin",
            Some(OsStr::new("different-fixture-password")),
            true
        )
        .await
        .status
        .success()
    );
    assert_eq!(users(&db).await, before);
    assert!(
        !init(&db, "replacement-admin", Some(OsStr::new(PASSWORD)), false)
            .await
            .status
            .success()
    );
    assert_eq!(users(&db).await, before);
}

#[tokio::test]
async fn existing_disabled_user_skips_missing_password_source() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("bootstrap.db");
    assert!(
        init(&db, "fixture-user", Some(OsStr::new(PASSWORD)), true)
            .await
            .status
            .success()
    );
    let pool = pool(&db).await;
    sqlx::query("UPDATE users SET role = 'user', enabled = 0")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let before = users(&db).await;
    assert!(init(&db, "", None, true).await.status.success());
    assert_eq!(users(&db).await, before);
}

#[tokio::test]
async fn invalid_or_partial_credentials_leave_no_user() {
    for (username, password) in [
        ("", Some(PASSWORD)),
        ("fixture-admin", None),
        ("fixture-admin", Some("short")),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("bootstrap.db");
        assert!(
            !init(&db, username, password.map(OsStr::new), true)
                .await
                .status
                .success()
        );
        assert!(users(&db).await.is_empty());
    }
}

#[tokio::test]
async fn concurrent_bootstrap_processes_create_only_one_user() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("bootstrap.db");
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        Command::new(BIN)
            .args(["migrate", "--db-path"])
            .arg(&db)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(output.status.success());
    let (first, second) = tokio::join!(
        init(&db, "fixture-first", Some(OsStr::new(PASSWORD)), true),
        init(&db, "fixture-second", Some(OsStr::new(PASSWORD)), true)
    );
    assert!(first.status.success() && second.status.success());
    let rows = users(&db).await;
    assert_eq!(rows.len(), 1);
    assert!(deve_sub_security::verify_password(PASSWORD, &rows[0].1).unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn non_utf8_password_error_is_redacted() {
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("bootstrap.db");
    let secret = OsStr::from_bytes(b"fixture-private-marker-\xff");
    let output = init(&db, "fixture-admin", Some(secret), true).await;
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("UTF-8"));
    assert!(!stderr.contains("fixture-private-marker"));
    assert!(users(&db).await.is_empty());
}
