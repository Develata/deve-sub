//! DEPLOY-001/PERF-002: forward index migration, lookup plan and old-backup recovery.
#![allow(clippy::expect_used)]

use deve_sub_storage_sqlite::{SqliteConfig, create_pool};

async fn lookup_plan(pool: &sqlx::SqlitePool) -> String {
    let rows: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN SELECT id FROM nodes WHERE identity_fingerprint = 'test' AND missing_from_source = 0",
    ).fetch_all(pool).await.expect("explain");
    rows.into_iter()
        .map(|row| row.3)
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn forward_index_removes_scan_and_backup_can_reupgrade() {
    let dir = tempfile::tempdir().expect("directory");
    let pool = create_pool(&SqliteConfig::new(dir.path().join("old.db")))
        .await
        .expect("pool");
    let mut old = sqlx::migrate!("../../migrations");
    old.migrations = old
        .migrations
        .iter()
        .filter(|m| m.version <= 25)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    old.run(&pool).await.expect("schema 25");
    sqlx::query("INSERT INTO nodes (id, protocol_kind, host, port, identity_fingerprint) VALUES ('n', 'trojan', 'test.example', 443, 'test')")
        .execute(&pool).await.expect("node");
    assert!(lookup_plan(&pool).await.contains("SCAN nodes"));
    let backup = dir.path().join("backup.db");
    sqlx::query("VACUUM INTO ?")
        .bind(backup.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .expect("backup");
    let restored = create_pool(&SqliteConfig::new(&backup))
        .await
        .expect("restore");
    for db in [&pool, &restored] {
        sqlx::migrate!("../../migrations")
            .run(db)
            .await
            .expect("forward upgrade");
        assert!(
            lookup_plan(db)
                .await
                .contains("SEARCH nodes USING COVERING INDEX idx_nodes_identity_lookup")
        );
        let node: String =
            sqlx::query_scalar("SELECT id FROM nodes WHERE identity_fingerprint='test'")
                .fetch_one(db)
                .await
                .expect("preserved node");
        assert_eq!(node, "n");
    }
}
