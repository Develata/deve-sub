//! SRC-001/GEN-015: source mutations and pool cache invalidation commit together.
use super::*;
use deve_sub_domain::PoolMetaRepository;
use deve_sub_storage_sqlite::SqlitePoolMetaRepository;

#[tokio::test]
async fn src001_source_changes_invalidate_pool_and_rollback_together() {
    let db = TestDb::new().await;
    let repo = SqliteSourceRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let pool = SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let revision = SqlitePoolMetaRepository::new(db.pool.clone());
    let mut source = make_source("before");
    repo.create(&source).await.expect("source");
    pool.reconcile(ReconcileInput {
        job_id: None,
        source_id: source.id,
        snapshot: &make_snapshot(source.id, 1, 1),
        entries: &[entry(trojan_node(TROJAN_A))],
    })
    .await
    .expect("refresh");
    let original = revision.get_revision().await.expect("revision");
    source.name = "after".into();
    repo.update(&source).await.expect("rename");
    let renamed = revision.get_revision().await.expect("revision");
    assert!(
        renamed > original,
        "a source label change invalidates generation"
    );
    sqlx::query("CREATE TRIGGER fail_revision BEFORE UPDATE ON pool_meta BEGIN SELECT RAISE(ABORT, 'fixture revision failure'); END")
        .execute(&db.pool).await.expect("inject write failure");
    source.name = "must-roll-back".into();
    assert!(repo.update(&source).await.is_err());
    assert_eq!(
        repo.find_by_id(source.id)
            .await
            .expect("source")
            .expect("exists")
            .name,
        "after"
    );
    assert!(repo.delete(source.id).await.is_err());
    assert!(repo.find_by_id(source.id).await.expect("source").is_some());
    assert_eq!(count_bindings(&db.pool, &source.id.to_string()).await, 1);
    assert_eq!(revision.get_revision().await.expect("revision"), renamed);
    sqlx::query("DROP TRIGGER fail_revision")
        .execute(&db.pool)
        .await
        .expect("restore writes");
    repo.delete(source.id).await.expect("delete");
    let deleted = revision.get_revision().await.expect("revision");
    assert!(deleted > renamed, "source deletion invalidates generation");
    assert_eq!(count_bindings(&db.pool, &source.id.to_string()).await, 0);
    assert!(repo.delete(source.id).await.is_err());
    assert!(repo.update(&source).await.is_err());
    assert_eq!(
        revision.get_revision().await.expect("revision"),
        deleted,
        "missing-source failures do not advance the pool"
    );
}
