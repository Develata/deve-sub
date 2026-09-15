//! GEN-006/015: selection boundaries, valid containers and cache upgrade safety.

use super::*;
use deve_sub_application::template::{
    TemplateAppError, UpdateTemplateParams, preview, update_template,
};
use deve_sub_domain::{
    GenerationCacheEntry, GenerationCacheRepository, NodeOverrideRepository, NodeSelector,
    PoolMetaRepository,
};
use deve_sub_kernel::{GenerationCacheId, NodeId};
use serde_json::{Value, json};

mod source_mutation;
mod withdrawal_groups;

fn document(selector: Value, groups: Value) -> String {
    json!({"apiVersion":"deve-sub.io/v1", "kind":"SubscriptionTemplate",
        "metadata":{"name":"safety", "version":1},
        "spec":{"targetProfiles":["mihomo"], "nodeSelector":selector,
            "proxyGroups":groups, "rules":[]}})
    .to_string()
}

async fn run(
    db: &TestDb,
    request: GenerationRequest,
    surface: &str,
) -> Result<deve_sub_domain::GenerationResult, TemplateAppError> {
    let templates = SqliteTemplateRepository::new(db.pool.clone());
    let versions = SqliteTemplateVersionRepository::new(db.pool.clone());
    let nodes = SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let cache = SqliteGenerationCacheRepository::new(db.pool.clone());
    let revision = SqlitePoolMetaRepository::new(db.pool.clone());
    match surface {
        "generate" => generate(&templates, &versions, &nodes, &cache, &revision, request).await,
        "delivery" => {
            generate_for_delivery(&templates, &versions, &nodes, &cache, &revision, request).await
        }
        "preview" => preview(&templates, &versions, &nodes, &cache, &revision, request).await,
        _ => panic!("unknown test surface"),
    }
}

#[tokio::test]
async fn gen006_groups_cannot_expand_fixed_or_dynamic_selection() {
    for selector in [
        json!({"mode":"dynamic", "filters":[{"field":"region", "value":"US"}]}),
        json!({"mode":"fixed", "nodeIds":[TROJAN_ID_A, TROJAN_ID_A]}),
    ] {
        let groups = json!([
            {"name":"explicit", "type":"select", "members":[{"kind":"node", "id":TROJAN_ID_A}, {"kind":"node", "id":TROJAN_ID_B}]},
            {"name":"quick", "type":"select", "filter":{"protocol":"trojan"}},
            {"name":"nested", "type":"select", "members":[{"kind":"group", "name":"quick"}]}
        ]);
        let db = TestDb::new(&document(selector.clone(), groups), "selection-boundary").await;
        deve_sub_storage_sqlite::SqliteNodeOverrideRepository::new(db.pool.clone())
            .patch_override_region(NodeId::parse(TROJAN_ID_A).expect("node"), Some("US".into()))
            .await
            .expect("region");
        let request = make_request(db.template_id, "mihomo");
        let result = run(&db, request.clone(), "generate")
            .await
            .expect("selected config");
        let output: serde_yaml::Value = serde_yaml::from_str(&result.content).expect("yaml");
        assert_eq!(
            output["proxies"].as_sequence().expect("proxies").len(),
            1,
            "groups must not add nodes outside the selector"
        );
        assert!(!result.content.contains("bravo-node"));
        assert_eq!(
            result.included_node_ids.len(),
            1,
            "fixed IDs retain set semantics"
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("outside_selection"))
        );
        let mut delivery = request;
        delivery.node_selection = Some(serde_json::from_value(selector).expect("selector"));
        assert_eq!(
            run(&db, delivery, "delivery")
                .await
                .expect("delivery cache")
                .content,
            result.content
        );
    }
}

#[tokio::test]
async fn gen006_empty_selection_and_resolved_group_preserve_scope() {
    for selector in [
        json!({"mode":"fixed", "nodeIds":[]}),
        json!({"mode":"fixed", "nodeIds":[TROJAN_ID_A]}),
    ] {
        let empty = selector["nodeIds"].as_array().expect("ids").is_empty();
        let db = TestDb::new(&document(selector, json!([{"name":"only-outside", "type":"select", "members":[{"kind":"node", "id":TROJAN_ID_B}]}])), "empty-scope").await;
        let result = run(&db, make_request(db.template_id, "mihomo"), "generate").await;
        if !empty {
            let result = result.expect("safe blocked group with selected A");
            assert!(!result.content.contains("bravo-node"));
            let output: serde_yaml::Value = serde_yaml::from_str(&result.content).expect("yaml");
            assert_eq!(
                output["proxy-groups"][0]["proxies"],
                serde_yaml::to_value(["REJECT"]).expect("members")
            );
            continue;
        }
        assert!(
            result.is_err(),
            "an empty selection cannot produce a valid scoped container"
        );
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
    }
}

#[tokio::test]
async fn gen015_mihomo_unsupported_groups_never_replace_last_good() {
    let db = TestDb::new(SPEC_MIHOMO_ONLY, "group-validation").await;
    let request = make_request(db.template_id, "mihomo");
    let good = run(&db, request.clone(), "generate")
        .await
        .expect("good generation");
    for group_type in ["direct", "reject"] {
        update_template(&SqliteTemplateRepository::new(db.pool.clone()), UpdateTemplateParams {
            id: db.template_id, name: "group-validation".into(), description: "test".into(),
            spec_yaml: document(json!({"mode":"dynamic"}), json!([{"name":"unsupported", "type":group_type, "members":[{"kind":"node", "id":TROJAN_ID_A}]}])),
        }).await.expect("save structurally valid template");
        for mode in [GenerationMode::Lenient, GenerationMode::Strict] {
            for surface in ["generate", "preview"] {
                let mut request = request.clone();
                request.mode = mode;
                assert!(
                    matches!(
                        run(&db, request, surface).await,
                        Err(TemplateAppError::Generation(
                            deve_sub_domain::GenerationError::IncompatibleGroupTypes { .. }
                        ))
                    ),
                    "unsupported group must fail before publication"
                );
            }
        }
        let active = get_active_generation(
            &SqliteGenerationCacheRepository::new(db.pool.clone()),
            db.template_id,
            "mihomo",
        )
        .await
        .expect("lookup")
        .expect("good retained");
        assert_eq!(active.content, good.content);
        assert_eq!(
            run(&db, request.clone(), "delivery")
                .await
                .expect("last good fallback")
                .content,
            good.content
        );
    }
}

// Reproduce the exact pre-fix key encoding, independent of the new key builder.
fn legacy_key(entry: &GenerationCacheEntry, semantics: Option<&str>) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    if let Some(semantics) = semantics {
        hasher.update((semantics.len() as u64).to_le_bytes());
        hasher.update(semantics.as_bytes());
    }
    for value in [
        entry.template_id.to_string(),
        entry.template_version.to_string(),
        entry.profile.clone(),
        entry.mode.clone(),
        entry.selection_mode.clone(),
        entry.selection_payload.clone(),
        entry.pool_revision.to_string(),
    ] {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn legacy_cache(db: &TestDb) {
    seed_legacy_cache(db, None).await;
}

async fn seed_legacy_cache(db: &TestDb, semantics: Option<&str>) {
    let selector: NodeSelector =
        serde_json::from_value(json!({"mode":"dynamic"})).expect("selector");
    let mut entry = GenerationCacheEntry {
        id: GenerationCacheId::new(),
        template_id: db.template_id,
        template_version: 1,
        profile: "mihomo".into(),
        mode: "lenient".into(),
        selection_mode: "dynamic".into(),
        selection_payload: serde_json::to_string(&selector).expect("selector"),
        pool_revision: SqlitePoolMetaRepository::new(db.pool.clone())
            .get_revision()
            .await
            .expect("revision")
            .value(),
        cache_key: String::new(),
        content: "legacy-invalid-or-unselected-output".into(),
        is_active: false,
    };
    entry.cache_key = legacy_key(&entry, semantics);
    let repo = SqliteGenerationCacheRepository::new(db.pool.clone());
    repo.store(&entry).await.expect("legacy cache");
    repo.activate(db.template_id, "mihomo", entry.id)
        .await
        .expect("legacy active");
}

#[tokio::test]
async fn gen015_legacy_cache_hits_are_regenerated_on_every_surface() {
    for surface in ["generate", "preview", "delivery"] {
        let db = TestDb::new(SPEC_MIHOMO_ONLY, "legacy-hit").await;
        legacy_cache(&db).await;
        let result = run(&db, make_request(db.template_id, "mihomo"), surface)
            .await
            .expect("regenerated");
        assert!(
            result.content.contains("proxies:"),
            "old cache cannot bypass the corrected generator"
        );
    }
}

#[tokio::test]
async fn gen015_legacy_cache_is_not_a_last_good_fallback() {
    let db = TestDb::new(SPEC_MIHOMO_ONLY, "legacy-fallback").await;
    legacy_cache(&db).await;
    let ids = [TROJAN_ID_A, TROJAN_ID_B, TROJAN_ID_C].map(|id| NodeId::parse(id).expect("id"));
    deve_sub_storage_sqlite::SqliteNodeOverrideRepository::new(db.pool.clone())
        .batch_set_enabled(&ids, false)
        .await
        .expect("disable nodes");
    assert!(
        run(&db, make_request(db.template_id, "mihomo"), "delivery")
            .await
            .is_err(),
        "do not deliver unverified legacy cache when regeneration fails"
    );
    let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM generation_cache")
        .fetch_one(&db.pool)
        .await
        .expect("retained rows");
    assert_eq!(
        retained, 1,
        "refuse delivery without destructive cache cleanup"
    );
}

#[tokio::test]
async fn gen015_legacy_active_cache_is_not_exposed_as_valid_output() {
    let db = TestDb::new(SPEC_MIHOMO_ONLY, "legacy-active").await;
    legacy_cache(&db).await;
    assert!(
        get_active_generation(
            &SqliteGenerationCacheRepository::new(db.pool.clone()),
            db.template_id,
            "mihomo"
        )
        .await
        .expect("lookup")
        .is_none(),
        "old active cache must not expose unvalidated output"
    );
}
