//! Maintenance failure isolation for the production loop (AUDIT-005).
#![allow(clippy::expect_used)]

use super::*;
use deve_sub_domain::AuditLog;
use deve_sub_kernel::Timestamp;
use deve_sub_storage_sqlite::SqliteAuditLogRepository;

#[tokio::test]
async fn audit_005_receipt_failure_does_not_block_other_retention() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("test.db");
    let pool =
        deve_sub_storage_sqlite::create_pool(&deve_sub_storage_sqlite::SqliteConfig::new(&path))
            .await
            .expect("pool");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("migrate");
    let audit = SqliteAuditLogRepository::new(pool.clone());
    let mut event = AuditLog::new(None, "fixture.old", None, None, None);
    event.created_at = Timestamp::now() - time::Duration::days(100);
    audit.insert(&event).await.expect("event");
    sqlx::query("CREATE TRIGGER fixture_failure BEFORE INSERT ON audit_log WHEN NEW.action = 'audit.cleanup' BEGIN SELECT RAISE(ABORT, 'fixture'); END")
        .execute(&pool).await.expect("trigger");
    sqlx::query("INSERT INTO outbox_event (id, aggregate_type, aggregate_id, event_type, payload_json, processed_at) VALUES ('fixture', 'fixture', 'fixture', 'fixture', '{}', '2020-01-01T00:00:00Z')")
        .execute(&pool).await.expect("expired event");
    let maintenance = SqliteMaintenance::new(pool.clone(), &path);
    assert_eq!(prune_round(&maintenance, &audit, 90).await, 1);
    let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log")
        .fetch_one(&pool)
        .await
        .expect("audit count");
    let outbox_count: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_event")
        .fetch_one(&pool)
        .await
        .expect("outbox count");
    assert_eq!(audit_count, 1);
    assert_eq!(outbox_count, 0);
    sqlx::query("DROP TRIGGER fixture_failure")
        .execute(&pool)
        .await
        .expect("recovery");
    assert_eq!(prune_round(&maintenance, &audit, 90).await, 1);
}
