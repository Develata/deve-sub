//! AUDIT-004/005: bounded cleanup, concurrent writers, and transactional recovery.
#![allow(clippy::expect_used)]

use deve_sub_application::audit;
use deve_sub_domain::{AuditError, AuditLog, AuditLogFilter, AuditLogRepository};
use deve_sub_kernel::Timestamp;
use deve_sub_storage_sqlite::SqliteAuditLogRepository;
use sqlx::SqlitePool;

async fn database() -> (tempfile::TempDir, SqlitePool, SqliteAuditLogRepository) {
    let dir = tempfile::tempdir().expect("directory");
    let config = deve_sub_storage_sqlite::SqliteConfig::new(dir.path().join("test.db"));
    let pool = deve_sub_storage_sqlite::create_pool(&config)
        .await
        .expect("pool");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("migrate");
    let repo = SqliteAuditLogRepository::new(pool.clone());
    (dir, pool, repo)
}

fn event(days_old: i64) -> AuditLog {
    let mut entry = AuditLog::new(None, "fixture.action", None, None, None);
    entry.created_at = Timestamp::now() - time::Duration::days(days_old);
    entry
}

async fn seed(repo: &SqliteAuditLogRepository, count: usize, days_old: i64) {
    for _ in 0..count {
        repo.insert(&event(days_old)).await.expect("insert");
    }
}

async fn count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_log")
        .fetch_one(pool)
        .await
        .expect("count")
}

#[tokio::test]
async fn audit_004_preview_is_read_only_and_cleanup_is_bounded() {
    let (_dir, pool, repo) = database().await;
    seed(&repo, 502, 100).await;
    seed(&repo, 2, 0).await;
    let preview = audit::preview_cleanup(&repo, 90).await.expect("preview");
    assert_eq!(preview.entry_ids.len(), 500);
    assert!(preview.has_more);
    assert_eq!(count(&pool).await, 504);
    let receipt = audit::cleanup(&repo, preview.before, &preview.entry_ids, None)
        .await
        .expect("cleanup");
    assert_eq!(count(&pool).await, 5);
    let entries = repo
        .list(
            &AuditLogFilter {
                action: Some("audit.cleanup".into()),
                ..Default::default()
            },
            None,
            100,
        )
        .await
        .expect("receipt");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, receipt);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            entries[0].details_json.as_ref().expect("details")
        )
        .expect("json")["deleted"],
        500
    );
    assert!(matches!(
        audit::cleanup(&repo, preview.before, &preview.entry_ids, None).await,
        Err(AuditError::Conflict)
    ));
    assert_eq!(count(&pool).await, 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn audit_004_concurrent_confirmations_and_writes_preserve_new_events() {
    let (_dir, pool, repo) = database().await;
    seed(&repo, 40, 100).await;
    let preview = audit::preview_cleanup(&repo, 90).await.expect("preview");
    let barrier = tokio::sync::Barrier::new(3);
    let confirm = async {
        barrier.wait().await;
        audit::cleanup(&repo, preview.before, &preview.entry_ids, None).await
    };
    let confirm2 = async {
        barrier.wait().await;
        audit::cleanup(&repo, preview.before, &preview.entry_ids, None).await
    };
    let writer = async {
        barrier.wait().await;
        seed(&repo, 25, 0).await;
    };
    let (a, b, ()) = tokio::join!(confirm, confirm2, writer);
    assert!(matches!(
        (&a, &b),
        (Ok(_), Err(AuditError::Conflict)) | (Err(AuditError::Conflict), Ok(_))
    ));
    assert_eq!(count(&pool).await, 26);
    assert_eq!(
        audit::prune_audit_logs(&repo, 90)
            .await
            .expect("nothing expired"),
        0
    );
}

#[tokio::test]
async fn audit_004_receipt_failure_rolls_back_deletion() {
    let (_dir, pool, repo) = database().await;
    seed(&repo, 3, 100).await;
    sqlx::query("CREATE TRIGGER fixture_receipt_failure BEFORE INSERT ON audit_log WHEN NEW.action = 'audit.cleanup' BEGIN SELECT RAISE(ABORT, 'fixture receipt failure'); END")
        .execute(&pool).await.expect("trigger");
    let preview = audit::preview_cleanup(&repo, 90).await.expect("preview");
    assert!(matches!(
        audit::cleanup(&repo, preview.before, &preview.entry_ids, None).await,
        Err(AuditError::Storage(_))
    ));
    assert_eq!(count(&pool).await, 3);
    sqlx::query("DROP TRIGGER fixture_receipt_failure")
        .execute(&pool)
        .await
        .expect("recover");
    audit::cleanup(&repo, preview.before, &preview.entry_ids, None)
        .await
        .expect("retry");
    assert_eq!(count(&pool).await, 1);
}

#[tokio::test]
async fn audit_005_retention_disable_cutoff_and_unrelated_state() {
    let (_dir, pool, repo) = database().await;
    seed(&repo, 3, 100).await;
    assert_eq!(
        audit::prune_audit_logs(&repo, 0).await.expect("disabled"),
        0
    );
    assert_eq!(count(&pool).await, 3);
    let preview = audit::preview_cleanup(&repo, 90).await.expect("preview");
    let mut boundary = event(90);
    boundary.created_at = preview.before;
    let boundary_id = boundary.id;
    repo.insert(&boundary).await.expect("boundary");
    sqlx::query("INSERT INTO outbox_event (id, aggregate_type, aggregate_id, event_type, payload_json) VALUES ('fixture', 'fixture', 'fixture', 'fixture', '{}')")
        .execute(&pool).await.expect("unrelated");
    audit::cleanup(&repo, preview.before, &preview.entry_ids, None)
        .await
        .expect("cleanup");
    let entries = repo
        .list(&AuditLogFilter::default(), None, 100)
        .await
        .expect("list");
    assert!(entries.iter().any(|entry| entry.id == boundary_id));
    let outbox: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_event")
        .fetch_one(&pool)
        .await
        .expect("unrelated");
    assert_eq!(outbox, 1);
    seed(&repo, 2, 100).await;
    assert_eq!(audit::prune_audit_logs(&repo, 91).await.expect("auto"), 2);
    assert_eq!(audit::prune_audit_logs(&repo, 91).await.expect("no-op"), 0);
}

#[tokio::test]
async fn audit_004_invalid_scope_and_changed_preview_do_not_delete() {
    let (_dir, pool, repo) = database().await;
    seed(&repo, 2, 100).await;
    for days in [0, 3651, u32::MAX] {
        assert!(matches!(
            audit::preview_cleanup(&repo, days).await,
            Err(AuditError::Invalid(_))
        ));
    }
    let p = audit::preview_cleanup(&repo, 90).await.expect("preview");
    for ids in [vec![], vec![p.entry_ids[0]; 2], vec![p.entry_ids[0]; 501]] {
        assert!(matches!(
            audit::cleanup(&repo, p.before, &ids, None).await,
            Err(AuditError::Invalid(_))
        ));
    }
    assert!(matches!(
        audit::cleanup(&repo, Timestamp::now(), &p.entry_ids, None).await,
        Err(AuditError::Invalid(_))
    ));
    // A backdated append changes the exact preview and must be reviewed again.
    seed(&repo, 1, 200).await;
    assert!(matches!(
        audit::cleanup(&repo, p.before, &p.entry_ids, None).await,
        Err(AuditError::Conflict)
    ));
    assert_eq!(count(&pool).await, 3);
}

#[tokio::test]
async fn audit_005_migration_index_and_backup_recovery() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("upgrade.db");
    let config = deve_sub_storage_sqlite::SqliteConfig::new(&path);
    let pool = deve_sub_storage_sqlite::create_pool(&config)
        .await
        .expect("pool");
    let mut before = sqlx::migrate!("../../migrations");
    before.migrations = before
        .iter()
        .filter(|m| m.version < 27)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    before.run(&pool).await.expect("old schema");
    let repo = SqliteAuditLogRepository::new(pool.clone());
    seed(&repo, 3, 100).await;
    let backup = dir.path().join("backup.db");
    sqlx::query("VACUUM INTO ?")
        .bind(backup.to_str().expect("path"))
        .execute(&pool)
        .await
        .expect("backup");
    for _ in 0..2 {
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("upgrade rerun");
    }
    let plan: Vec<(i64, i64, i64, String)> = sqlx::query_as("EXPLAIN QUERY PLAN SELECT id FROM audit_log WHERE created_at < '2025-01-01T00:00:00Z' ORDER BY created_at, id LIMIT 501")
        .fetch_all(&pool).await.expect("plan");
    assert!(
        plan.iter()
            .any(|row| row.3.contains("idx_audit_log_retention"))
    );
    assert_eq!(count(&pool).await, 3);
    pool.close().await;
    // Recover to a different isolated file, never overwrite the source backup.
    let restored = dir.path().join("restored.db");
    std::fs::copy(&backup, &restored).expect("restore");
    let recovered =
        deve_sub_storage_sqlite::create_pool(&deve_sub_storage_sqlite::SqliteConfig::new(restored))
            .await
            .expect("reopen");
    assert_eq!(count(&recovered).await, 3);
    sqlx::migrate!("../../migrations")
        .run(&recovered)
        .await
        .expect("upgrade after restore");
    assert_eq!(count(&recovered).await, 3);
}

#[tokio::test]
async fn audit_004_cancelled_lock_wait_can_retry_without_partial_deletion() {
    let (_dir, pool, repo) = database().await;
    seed(&repo, 2, 100).await;
    let preview = audit::preview_cleanup(&repo, 90).await.expect("preview");
    let lock = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .expect("writer lock");
    let attempt = tokio::time::timeout(
        std::time::Duration::from_millis(30),
        audit::cleanup(&repo, preview.before, &preview.entry_ids, None),
    )
    .await;
    assert!(attempt.is_err(), "cancel while waiting for the writer lock");
    lock.rollback().await.expect("unlock");
    assert_eq!(count(&pool).await, 2);
    tokio::time::timeout(
        std::time::Duration::from_secs(6),
        audit::cleanup(&repo, preview.before, &preview.entry_ids, None),
    )
    .await
    .expect("bounded retry")
    .expect("cleanup");
    assert_eq!(count(&pool).await, 1);
}
