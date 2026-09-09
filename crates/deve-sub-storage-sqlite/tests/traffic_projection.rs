//! Upgrade, rollback, retention and atomic accounting regression evidence.
#![allow(clippy::expect_used)]

use deve_sub_storage_sqlite::{SqliteConfig, SqliteMaintenance, create_pool};
use sqlx::SqlitePool;

const SUB: &str = "00000000000000000000000001";

async fn old_database() -> (tempfile::TempDir, SqlitePool) {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = create_pool(&SqliteConfig::new(dir.path().join("test.db")))
        .await
        .expect("pool");
    let mut migrator = sqlx::migrate!("../../migrations");
    migrator.migrations = migrator
        .migrations
        .iter()
        .filter(|m| m.version <= 23)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    migrator.run(&pool).await.expect("old migrations");
    sqlx::raw_sql("INSERT INTO users (id, username, password_hash) VALUES ('u', 'test', 'TEST_HASH');
        INSERT INTO templates (id, name) VALUES ('t', 'test');
        INSERT INTO subscriptions (id, name, slug, owner_id, template_id, profile, node_selection, token_id)
        VALUES ('00000000000000000000000001', 's', 's', 'u', 't', 'mihomo', '{}', 'TEST_TOKEN_ID');")
        .execute(&pool).await.expect("seed parents");
    (dir, pool)
}

async fn insert(pool: &SqlitePool, id: &str, amount: i64, when: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO subscription_traffic (id, subscription_id, source_kind, upload, download, recorded_at, source_ref) VALUES (?, ?, 'P', ?, 2, ?, 'nezha:test')")
        .bind(id).bind(SUB).bind(amount).bind(when).execute(pool).await?;
    Ok(())
}
async fn scalar(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("query")
}

#[tokio::test]
async fn upgrade_backfill_prune_and_backup_restore_preserve_totals() {
    let (dir, pool) = old_database().await;
    insert(&pool, "old", 100, "2025-01-01T00:00:00Z")
        .await
        .expect("old sample");
    let backup = dir.path().join("before.db");
    sqlx::query("VACUUM INTO ?")
        .bind(backup.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .expect("backup");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("upgrade");
    assert_eq!(
        scalar(&pool, "SELECT upload FROM traffic_totals").await,
        100
    );
    assert_eq!(
        scalar(&pool, "SELECT total_upload FROM traffic_daily_snapshots").await,
        100
    );
    assert_eq!(
        scalar(&pool, "SELECT upload FROM probe_traffic_totals").await,
        100
    );
    let now: String = sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')")
        .fetch_one(&pool)
        .await
        .expect("now");
    insert(&pool, "new", 50, &now).await.expect("new sample");
    let maintenance = SqliteMaintenance::new(pool.clone(), dir.path().join("test.db"));
    maintenance.prune_history().await.expect("prune");
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM subscription_traffic").await,
        1
    );
    assert_eq!(
        scalar(&pool, "SELECT upload FROM traffic_totals").await,
        150
    );
    assert_eq!(
        scalar(&pool, "SELECT upload FROM probe_traffic_totals").await,
        150
    );
    let restored = create_pool(&SqliteConfig::new(&backup))
        .await
        .expect("restore old backup");
    assert_eq!(
        scalar(&restored, "SELECT COUNT(*) FROM subscription_traffic").await,
        1
    );
    assert_eq!(
        scalar(&restored, "SELECT MAX(version) FROM _sqlx_migrations").await,
        23
    );
    sqlx::migrate!("../../migrations")
        .run(&restored)
        .await
        .expect("upgrade restored backup");
    assert_eq!(
        scalar(&restored, "SELECT upload FROM traffic_totals").await,
        100
    );
}

#[tokio::test]
async fn invalid_old_data_aborts_migration_and_can_retry() {
    let (_dir, pool) = old_database().await;
    insert(&pool, "bad", -1, "2025-01-01T00:00:00Z")
        .await
        .expect("legacy invalid row");
    assert!(sqlx::migrate!("../../migrations").run(&pool).await.is_err());
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'traffic_totals'"
        )
        .await,
        0
    );
    assert_eq!(
        scalar(&pool, "SELECT MAX(version) FROM _sqlx_migrations").await,
        23
    );
    sqlx::query("DELETE FROM subscription_traffic WHERE id = 'bad'")
        .execute(&pool)
        .await
        .expect("fixture repair");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("retry");
}

#[tokio::test]
async fn duplicate_overflow_and_rollback_never_partially_project() {
    let (_dir, pool) = old_database().await;
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("upgrade");
    insert(&pool, "max", i64::MAX, "2026-01-01T00:00:00Z")
        .await
        .expect("max");
    assert!(
        insert(&pool, "overflow", 1, "2026-01-01T00:00:00Z")
            .await
            .is_err()
    );
    assert!(
        insert(&pool, "max", 0, "2026-01-01T00:00:00Z")
            .await
            .is_err()
    );
    assert!(
        insert(&pool, "negative", -1, "2026-01-01T00:00:00Z")
            .await
            .is_err()
    );
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM subscription_traffic").await,
        1
    );
    assert_eq!(
        scalar(&pool, "SELECT upload FROM traffic_totals").await,
        i64::MAX
    );
    assert_eq!(
        scalar(&pool, "SELECT total_download FROM traffic_daily_snapshots").await,
        2
    );
    let mut tx = pool.begin().await.expect("transaction");
    sqlx::query("INSERT INTO subscription_traffic (id, subscription_id, source_kind, upload, download) VALUES ('rolled-back', ?, 'M', 1, 1)")
        .bind(SUB).execute(&mut *tx).await.expect("insert in transaction");
    tx.rollback().await.expect("rollback");
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM traffic_totals").await,
        1
    );
}

#[tokio::test]
async fn cleanup_is_bounded_and_preserves_active_work() {
    let (dir, pool) = old_database().await;
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("upgrade");
    sqlx::raw_sql("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<1200)
        INSERT INTO subscription_traffic (id, subscription_id, source_kind, upload, recorded_at)
        SELECT 'sample-' || i, '00000000000000000000000001', 'M', 1, '2025-01-01T00:00:00Z' FROM n;
        INSERT INTO probe_runs (id, probe_type, node_ids, status, created_at) VALUES ('active', 'T', '[]', 'R', '2025-01-01T00:00:00Z');
        INSERT INTO outbox_event (id, aggregate_type, aggregate_id, event_type, payload_json, created_at) VALUES ('pending', 't', 't', 't', '{}', '2025-01-01T00:00:00Z');")
        .execute(&pool).await.expect("seed");
    let maintenance = SqliteMaintenance::new(pool.clone(), dir.path().join("test.db"));
    maintenance.prune_history().await.expect("prune");
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM subscription_traffic").await,
        700
    );
    assert_eq!(
        scalar(&pool, "SELECT upload FROM traffic_totals").await,
        1200
    );
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM probe_runs").await, 1);
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM outbox_event").await, 1);
}
