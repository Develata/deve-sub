//! GEN-015: source-label mutations must not bypass fresh resolution via a cache hit.
use super::*;
use deve_sub_domain::{
    ItemParseStatus, ReconcileEntry, ReconcileInput, Source, SourceRepository, SourceSnapshot,
    SourceType,
};
use deve_sub_kernel::SourceSnapshotId;
use deve_sub_storage_sqlite::SqliteSourceRepository;

async fn source_mutation(action: &str) {
    let db = TestDb::new(
        &document(
            json!({"mode":"dynamic", "filters":[{"field":"source", "value":"before"}]}),
            json!([]),
        ),
        "source-cache",
    )
    .await;
    let sources = SqliteSourceRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let nodes = SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let mut source = Source::new(
        "before",
        SourceType::UriList,
        "https://source.example.com/sub".into(),
    );
    sources.create(&source).await.expect("source");
    let node = deve_sub_protocol::parse_uri("trojan://fixture@remote.example.com:443#remote")
        .expect("node");
    nodes
        .reconcile(ReconcileInput {
            job_id: None,
            source_id: source.id,
            snapshot: &SourceSnapshot {
                id: SourceSnapshotId::new(),
                source_id: source.id,
                version: 1,
                fetched_at: Timestamp::now(),
                etag: None,
                node_count: 1,
                is_active: true,
            },
            entries: &[ReconcileEntry {
                raw_uri: "fixture".into(),
                initial_status: ItemParseStatus::Parsed,
                node: Some(node),
            }],
        })
        .await
        .expect("refresh");
    let request = make_request(db.template_id, "mihomo");
    let good = run(&db, request.clone(), "generate")
        .await
        .expect("good config");
    assert!(good.content.contains("remote.example.com"));
    assert_eq!(good.included_node_ids.len(), 1);
    if action == "rename" {
        source.name = "after".into();
        sources.update(&source).await.expect("rename");
    } else {
        sources.delete(source.id).await.expect("delete");
    }
    for surface in ["generate", "preview"] {
        assert!(
            matches!(
                run(&db, request.clone(), surface).await,
                Err(TemplateAppError::NoCompatibleNodes)
            ),
            "the old source label no longer matches; cached content is not a fresh result"
        );
    }
    if action == "delete" {
        assert!(
            matches!(
                run(&db, request, "delivery").await,
                Err(TemplateAppError::NoCompatibleNodes)
            ),
            "explicit withdrawal cannot be undone by fallback"
        );
        return;
    }
    let fallback = run(&db, request, "delivery")
        .await
        .expect("current last-good fallback");
    assert_eq!(fallback.content, good.content);
    assert!(
        fallback
            .warnings
            .iter()
            .any(|w| w.contains("served last successful")),
        "delivery must explicitly report failure fallback, not a stale cache hit"
    );
}

#[tokio::test]
async fn gen015_source_rename_invalidates_direct_generation_cache() {
    source_mutation("rename").await;
}
#[tokio::test]
async fn gen015_source_delete_invalidates_direct_generation_cache() {
    source_mutation("delete").await;
}

#[tokio::test]
async fn gen015_v2_v3_caches_are_regenerated_and_never_used_as_fallback() {
    for salt in ["deve-sub-generation-v2", "deve-sub-generation-v3"] {
        for surface in ["generate", "preview", "delivery"] {
            let db = TestDb::new(SPEC_MIHOMO_ONLY, "v2-hit").await;
            seed_legacy_cache(&db, Some(salt)).await;
            let result = run(&db, make_request(db.template_id, "mihomo"), surface)
                .await
                .expect("regenerate");
            assert!(
                result.content.contains("proxies:"),
                "pre-withdrawal semantics must be rebuilt"
            );
        }
        let db = TestDb::new(SPEC_MIHOMO_ONLY, "v2-fallback").await;
        seed_legacy_cache(&db, Some(salt)).await;
        let ids = [TROJAN_ID_A, TROJAN_ID_B, TROJAN_ID_C].map(|id| NodeId::parse(id).expect("id"));
        deve_sub_storage_sqlite::SqliteNodeOverrideRepository::new(db.pool.clone())
            .batch_set_enabled(&ids, false)
            .await
            .expect("disable");
        assert!(
            get_active_generation(
                &SqliteGenerationCacheRepository::new(db.pool.clone()),
                db.template_id,
                "mihomo"
            )
            .await
            .expect("active")
            .is_none()
        );
        assert!(
            run(&db, make_request(db.template_id, "mihomo"), "delivery")
                .await
                .is_err()
        );
    }
}
