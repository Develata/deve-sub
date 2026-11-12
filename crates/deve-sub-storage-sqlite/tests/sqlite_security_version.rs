#![allow(clippy::expect_used)]

use deve_sub_storage_sqlite::{SqliteConfig, create_pool};

#[tokio::test]
async fn linked_sqlite_contains_wal_page_size_security_fix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = create_pool(&SqliteConfig::new(dir.path().join("version.db")))
        .await
        .expect("pool");
    let version: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&pool)
        .await
        .expect("linked SQLite version");
    let parts: Vec<u32> = version
        .split('.')
        .map(|part| part.parse().expect("numeric SQLite version"))
        .collect();
    assert_eq!(parts.len(), 3, "unexpected SQLite version: {version}");
    // WHY: a lockfile update alone cannot prove which SQLite was linked.
    // RUSTSEC-2026-0459 requires SQLite 3.54.0 or newer for bundled builds.
    assert!(
        (parts[0], parts[1], parts[2]) >= (3, 54, 0),
        "SQLite {version} predates the WAL page-size security fix"
    );
    println!("linked SQLite version: {version}");
    pool.close().await;
}
