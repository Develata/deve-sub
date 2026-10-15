//! DEPLOY-001/PERF-002: forward index migration, lookup plan and old-backup recovery.
#![allow(clippy::expect_used)]

use deve_sub_storage_sqlite::{SqliteConfig, create_pool};

async fn lookup_plan(connection: &mut sqlx::SqliteConnection) -> String {
    // WHY: EXPLAIN does not execute the read that refreshes a connection's
    // schema after another pooled connection commits a migration. Execute a
    // real lookup first, then prepare EXPLAIN without its old statement cache.
    let _: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM nodes WHERE identity_fingerprint = 'test' AND missing_from_source = 0",
    )
    .fetch_all(&mut *connection)
    .await
    .expect("real lookup");
    let rows: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN SELECT id FROM nodes WHERE identity_fingerprint = 'test' AND missing_from_source = 0",
    ).persistent(false).fetch_all(connection).await.expect("explain");
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
    assert!(
        lookup_plan(&mut pool.acquire().await.expect("connection"))
            .await
            .contains("SCAN nodes")
    );
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
        // Keep the pre-migration reader checked out so migration must use a
        // different connection. The old test encountered this order by chance.
        let mut reader = db.acquire().await.expect("reader");
        assert!(lookup_plan(&mut reader).await.contains("SCAN nodes"));
        sqlx::migrate!("../../migrations")
            .run(db)
            .await
            .expect("forward upgrade");
        let plan = lookup_plan(&mut reader).await;
        assert!(
            plan.contains("SEARCH nodes USING COVERING INDEX idx_nodes_identity_lookup"),
            "{plan}"
        );
        let node: String =
            sqlx::query_scalar("SELECT id FROM nodes WHERE identity_fingerprint='test'")
                .fetch_one(db)
                .await
                .expect("preserved node");
        assert_eq!(node, "n");
    }
}
