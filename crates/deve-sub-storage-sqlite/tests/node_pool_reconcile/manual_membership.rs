//! NODE-001/011: source refresh cannot withdraw an independent manual import.
use super::*;
use deve_sub_domain::{ImportOutcome, NodeFilter, NodeOverride, NodeOverrideRepository};
use deve_sub_kernel::NodeOverrideId;
use deve_sub_storage_sqlite::SqliteNodeOverrideRepository;

async fn refresh(
    repo: &SqliteNodePoolRepository,
    source: SourceId,
    version: u64,
    with_a: bool,
) -> deve_sub_domain::ReconcileResult {
    let mut entries = vec![entry(trojan_node(TROJAN_B))];
    if with_a {
        entries.push(entry(trojan_node(TROJAN_A)));
    }
    repo.reconcile(ReconcileInput {
        job_id: None,
        source_id: source,
        snapshot: &make_snapshot(source, version, entries.len() as u64),
        entries: &entries,
    })
    .await
    .expect("refresh")
}

async fn import(repo: &SqliteNodePoolRepository) -> NodeId {
    let mut node = trojan_node(TROJAN_A);
    node.source.source_label = "manual".into();
    let result = repo.import_nodes(vec![node]).await.expect("manual import");
    match result.outcomes[0] {
        ImportOutcome::Inserted(id) | ImportOutcome::Duplicate(id) => id,
        _ => panic!("import must resolve an identity"),
    }
}

async fn scenario(order: &str) {
    let db = TestDb::new().await;
    let sources = SqliteSourceRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let repo = SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let source = make_source("remote");
    sources.create(&source).await.expect("source");
    let first = if order == "manual-first" {
        Some(import(&repo).await)
    } else {
        None
    };
    refresh(&repo, source.id, 1, true).await;
    let id = repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("nodes")
        .into_iter()
        .find(|n| n.node.display_name == "NodeA")
        .expect("A")
        .node
        .id;
    if let Some(first) = first {
        assert_eq!(first, id);
    }
    let overrides = SqliteNodeOverrideRepository::new(db.pool.clone());
    let tag = overrides
        .create_tag("independent", Some("#123456"))
        .await
        .expect("tag");
    overrides
        .set_node_tags(id, &[tag.id])
        .await
        .expect("membership");
    overrides
        .upsert_override(&NodeOverride {
            id: NodeOverrideId::new(),
            node_id: id,
            display_name: Some("manual-name".into()),
            region: Some("US".into()),
            enabled: Some(true),
            sni: Some("manual.example.com".into()),
            skip_cert_verify: Some(false),
            fingerprint: Some("chrome".into()),
            sort_order: 9,
        })
        .await
        .expect("override");
    let before = repo.get_node(id).await.expect("lookup").expect("node");
    if order == "missing-reimport" {
        refresh(&repo, source.id, 2, false).await;
    }
    if order == "concurrent" {
        let (imported, _) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(import(&repo), refresh(&repo, source.id, 2, false))
        })
        .await
        .expect("concurrent import/refresh");
        assert_eq!(imported, id);
    } else if order != "manual-first" {
        assert_eq!(import(&repo).await, id);
    }
    // Repeated disappearance also proves missing reimport establishes lasting provenance.
    for version in [3, 5] {
        refresh(&repo, source.id, version, true).await;
        let result = refresh(&repo, source.id, version + 1, false).await;
        assert_eq!(
            result.missing_nodes, 0,
            "manual contribution is still present"
        );
        let node = repo.get_node(id).await.expect("lookup").expect("node");
        assert!(!node.missing_from_source);
        assert_eq!(node.node.source.source_label, "manual");
        assert_eq!(node.tags, before.tags);
        assert_eq!(node.override_info, before.override_info);
        assert_eq!(node.node.authentication, before.node.authentication);
        assert_eq!(node.node.tls, before.node.tls);
        assert_eq!(
            repo.list_nodes(&NodeFilter::active_only(), None, 100)
                .await
                .expect("active")
                .len(),
            2
        );
    }
}

#[tokio::test]
async fn node011_manual_first_survives_remote_removal() {
    scenario("manual-first").await;
}
#[tokio::test]
async fn node011_source_first_manual_duplicate_survives_remote_removal() {
    scenario("source-first").await;
}
#[tokio::test]
async fn node011_missing_reimport_survives_later_remote_removal() {
    scenario("missing-reimport").await;
}
#[tokio::test]
async fn node011_concurrent_import_and_refresh_preserve_manual_membership() {
    scenario("concurrent").await;
}

#[tokio::test]
async fn node011_remote_named_manual_still_becomes_missing() {
    let db = TestDb::new().await;
    let sources = SqliteSourceRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let repo = SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let source = make_source("manual");
    sources.create(&source).await.expect("source");
    refresh(&repo, source.id, 1, true).await;
    assert_eq!(
        refresh(&repo, source.id, 2, false).await.missing_nodes,
        1,
        "display label is not independent provenance"
    );
}
