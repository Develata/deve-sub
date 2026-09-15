//! SRC-001/NODE-011: deletion withdraws ownership atomically with cache fencing.
use super::*;
use deve_sub_domain::{NodeFilter, NodeOverrideRepository, PoolMetaRepository};
use deve_sub_storage_sqlite::{SqliteNodeOverrideRepository, SqlitePoolMetaRepository};

async fn reconcile(repo: &SqliteNodePoolRepository, source: SourceId, uris: &[&str]) {
    let entries: Vec<_> = uris.iter().map(|uri| entry(trojan_node(uri))).collect();
    repo.reconcile(ReconcileInput {
        source_id: source,
        snapshot: &make_snapshot(source, 1, entries.len() as u64),
        entries: &entries,
    })
    .await
    .expect("refresh");
}

#[tokio::test]
async fn node011_delete_source_withdraws_only_its_exclusive_nodes() {
    let db = TestDb::new().await;
    let sources = SqliteSourceRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let pool = SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let a = make_source("A");
    let b = make_source("B");
    sources.create(&a).await.expect("A");
    sources.create(&b).await.expect("B");
    reconcile(&pool, a.id, &[TROJAN_A, TROJAN_B, TROJAN_A_ALT_PASS]).await;
    reconcile(&pool, b.id, &[TROJAN_B]).await;
    pool.import_nodes(vec![trojan_node(TROJAN_A_ALT_PASS)])
        .await
        .expect("independent duplicate");
    let before = pool
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("nodes");
    let exclusive = before
        .iter()
        .find(|n| n.node.display_name == "NodeA")
        .expect("A");
    let overrides = SqliteNodeOverrideRepository::new(db.pool.clone());
    let tag = overrides.create_tag("keep", None).await.expect("tag");
    overrides
        .set_node_tags(exclusive.node.id, &[tag.id])
        .await
        .expect("tag node");
    overrides
        .patch_override_region(exclusive.node.id, Some("US".into()))
        .await
        .expect("override");
    let preserved = pool
        .get_node(exclusive.node.id)
        .await
        .expect("node")
        .expect("exists");
    sources.delete(a.id).await.expect("delete A");
    let active = pool
        .list_nodes(&NodeFilter::active_only(), None, 100)
        .await
        .expect("active");
    assert_eq!(
        active.len(),
        2,
        "only shared and manual contributions survive"
    );
    assert!(active.iter().all(|n| n.node.id != exclusive.node.id));
    let withdrawn = pool
        .get_node(exclusive.node.id)
        .await
        .expect("node")
        .expect("diagnostic");
    assert!(withdrawn.missing_from_source);
    assert_eq!(withdrawn.tags, preserved.tags);
    assert_eq!(withdrawn.override_info, preserved.override_info);
    assert_eq!(withdrawn.node.authentication, preserved.node.authentication);
    sources.delete(b.id).await.expect("delete B");
    let active = pool
        .list_nodes(&NodeFilter::active_only(), None, 100)
        .await
        .expect("active");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].node.source.source_label, "manual");
}

#[tokio::test]
async fn node011_delete_source_rolls_back_with_failed_invalidation() {
    let db = TestDb::new().await;
    let sources = SqliteSourceRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let pool = SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let meta = SqlitePoolMetaRepository::new(db.pool.clone());
    let source = make_source("rollback");
    sources.create(&source).await.expect("source");
    reconcile(&pool, source.id, &[TROJAN_A]).await;
    let revision = meta.get_revision().await.expect("revision");
    sqlx::query("CREATE TRIGGER reject_revision BEFORE UPDATE ON pool_meta BEGIN SELECT RAISE(ABORT, 'fixture'); END")
        .execute(&db.pool).await.expect("inject");
    assert!(sources.delete(source.id).await.is_err());
    assert_eq!(count_nodes(&db.pool).await, (1, 0));
    assert_eq!(count_bindings(&db.pool, &source.id.to_string()).await, 1);
    assert!(
        sources
            .find_by_id(source.id)
            .await
            .expect("source")
            .is_some()
    );
    assert_eq!(meta.get_revision().await.expect("revision"), revision);
    sqlx::query("DROP TRIGGER reject_revision")
        .execute(&db.pool)
        .await
        .expect("restore");
    sources.delete(source.id).await.expect("delete");
    assert_eq!(count_nodes(&db.pool).await, (1, 1));
    let revision = meta.get_revision().await.expect("revision");
    assert!(sources.delete(source.id).await.is_err());
    assert_eq!(meta.get_revision().await.expect("revision"), revision);
}

#[tokio::test]
async fn node011_concurrent_delete_and_manual_import_preserve_identity() {
    let db = TestDb::new().await;
    let sources = SqliteSourceRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let pool = SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let source = make_source("concurrent");
    sources.create(&source).await.expect("source");
    reconcile(&pool, source.id, &[TROJAN_A]).await;
    let id = pool
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("nodes")[0]
        .node
        .id;
    let (deleted, imported) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(
            sources.delete(source.id),
            pool.import_nodes(vec![trojan_node(TROJAN_A)])
        )
    })
    .await
    .expect("bounded concurrent operations");
    deleted.expect("delete");
    imported.expect("import");
    let active = pool
        .list_nodes(&NodeFilter::active_only(), None, 100)
        .await
        .expect("active");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].node.id, id);
    assert_eq!(active[0].node.source.source_label, "manual");
}
