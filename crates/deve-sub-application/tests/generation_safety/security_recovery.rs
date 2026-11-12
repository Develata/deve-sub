//! Confirmed old-ID quarantine overrides all distribution surfaces and fallback.
use super::*;
use deve_sub_domain::GenerationCacheRepository;
use deve_sub_domain::source::recovery::{NodeSecurityAction, NodeSecurityChange, NodeSecurityPlan};

async fn quarantine(db: &TestDb, id: NodeId) {
    deve_sub_storage_sqlite::ensure_key_binding(
        &db.pool,
        &db.master_key.fingerprint().expect("key"),
    )
    .await
    .expect("bind original key");
    let repository = SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let old = repository
        .security_snapshot(id)
        .await
        .expect("old identity");
    let pool_revision: i64 = sqlx::query_scalar("SELECT revision FROM pool_meta WHERE id=1")
        .fetch_one(&db.pool)
        .await
        .expect("revision");
    repository
        .apply_security_plan(&NodeSecurityPlan {
            plan_sha256: "a".repeat(64),
            key_fingerprint: db.master_key.fingerprint().expect("key"),
            expected_pool_revision: pool_revision as u64,
            actions: vec![NodeSecurityAction {
                node_id: id,
                expected_revision: old.revision,
                expected_fingerprint: old.fingerprint,
                evidence_sha256: "b".repeat(64),
                change: NodeSecurityChange::Quarantine,
            }],
        })
        .await
        .expect("quarantine");
}

#[tokio::test]
async fn dynamic_and_quick_groups_regenerate_without_old_quarantined_cache_or_chain() {
    let spec = document(
        json!({"mode":"dynamic"}),
        json!([{"name":"quick","type":"select","filter":{"protocol":"trojan"}}]),
    );
    for surface in ["generate", "delivery", "preview"] {
        let db = TestDb::new(&spec, "security-dynamic").await;
        let repository =
            SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
        repository
            .set_node_chain(
                NodeId::parse(TROJAN_ID_C).expect("C"),
                Some(&[NodeId::parse(TROJAN_ID_A).expect("A")]),
            )
            .await
            .expect("chain");
        let old = run(&db, make_request(db.template_id, "mihomo"), "generate")
            .await
            .expect("old generation");
        assert!(old.content.contains("alpha-node"));
        let old_key: String = sqlx::query_scalar("SELECT cache_key FROM generation_cache LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .expect("old key");
        quarantine(&db, NodeId::parse(TROJAN_ID_A).expect("A")).await;
        let fresh = run(&db, make_request(db.template_id, "mihomo"), surface)
            .await
            .expect("unaffected remains available");
        assert!(!fresh.content.contains("alpha-node"));
        assert!(!fresh.content.contains("charlie-node"));
        assert!(fresh.content.contains("bravo-node"));
        let cache = SqliteGenerationCacheRepository::new(db.pool.clone(), db.master_key.clone());
        assert!(
            cache
                .find_by_key(&old_key)
                .await
                .expect("old direct")
                .is_none()
        );
    }
}

#[tokio::test]
async fn required_quarantined_node_and_chain_fail_in_both_modes_on_all_surfaces() {
    for required in [TROJAN_ID_A, TROJAN_ID_C] {
        for fixed in [true, false] {
            let selector = if fixed {
                json!({"mode":"fixed","nodeIds":[required,TROJAN_ID_B]})
            } else {
                json!({"mode":"dynamic"})
            };
            let groups = if fixed {
                json!([])
            } else {
                json!([{"name":"required","type":"select","members":[{"kind":"node","id":required},{"kind":"node","id":TROJAN_ID_B}]}])
            };
            let db = TestDb::new(&document(selector, groups), "security-required").await;
            SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone())
                .set_node_chain(
                    NodeId::parse(TROJAN_ID_C).expect("C"),
                    Some(&[NodeId::parse(TROJAN_ID_A).expect("A")]),
                )
                .await
                .expect("chain");
            quarantine(&db, NodeId::parse(TROJAN_ID_A).expect("A")).await;
            for surface in ["generate", "delivery", "preview"] {
                for mode in [GenerationMode::Strict, GenerationMode::Lenient] {
                    let mut request = make_request(db.template_id, "mihomo");
                    request.mode = mode;
                    let error = run(&db, request, surface)
                        .await
                        .expect_err("required security state must fail");
                    assert!(
                        error.to_string().contains("security_quarantined"),
                        "{surface}: {error}"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn quarantine_outside_the_selector_cannot_block_an_unaffected_selection() {
    for selector in [
        json!({"mode":"fixed","nodeIds":[TROJAN_ID_B]}),
        json!({"mode":"dynamic","filters":[{"field":"region","value":"US"}]}),
    ] {
        let groups = json!([{"name":"selected","type":"select","members":[{"kind":"node","id":TROJAN_ID_A},{"kind":"node","id":TROJAN_ID_B}]}]);
        let db = TestDb::new(&document(selector, groups), "security-outside-selection").await;
        deve_sub_storage_sqlite::SqliteNodeOverrideRepository::new(db.pool.clone())
            .patch_override_region(NodeId::parse(TROJAN_ID_B).expect("B"), Some("US".into()))
            .await
            .expect("region");
        quarantine(&db, NodeId::parse(TROJAN_ID_A).expect("A")).await;
        for surface in ["generate", "delivery", "preview"] {
            for mode in [GenerationMode::Strict, GenerationMode::Lenient] {
                let mut request = make_request(db.template_id, "mihomo");
                request.mode = mode;
                let result = run(&db, request, surface)
                    .await
                    .expect("unaffected selection remains available");
                assert!(result.content.contains("bravo-node"));
                assert!(!result.content.contains("alpha-node"));
            }
        }
    }
}

#[tokio::test]
async fn native_automatic_groups_keep_safe_nodes_but_explicit_required_quarantine_errors() {
    for explicit in [false, true] {
        let members = if explicit {
            format!("proxies: ['alpha-node [{TROJAN_ID_A}]', bravo-node]")
        } else {
            "include-all-proxies: true".into()
        };
        let spec = format!(
            "proxy-groups: [{{name: Native, type: select, {members}}}]\nrules: ['MATCH,Native']"
        );
        let db = TestDb::new(&spec, "security-native").await;
        quarantine(&db, NodeId::parse(TROJAN_ID_A).expect("A")).await;
        for surface in ["generate", "delivery", "preview"] {
            for mode in [GenerationMode::Strict, GenerationMode::Lenient] {
                let mut request = make_request(db.template_id, "mihomo");
                request.mode = mode;
                let result = run(&db, request, surface).await;
                if explicit {
                    assert!(
                        result
                            .expect_err("required old native ID")
                            .to_string()
                            .contains("security_quarantined")
                    );
                } else {
                    let result = result.expect("automatic safe branch");
                    assert!(!result.content.contains("alpha-node"));
                    assert!(result.content.contains("bravo-node"));
                }
            }
        }
    }
}
