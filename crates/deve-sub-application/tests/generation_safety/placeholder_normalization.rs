//! Native placeholders preserve selection, ordering, authored data and fallback.
use super::*;

const SPEC: &str = "mode: rule\nproxies: null\nproxy-groups:\n  - {name: G, type: select, proxies: [DIRECT, __PROXY_NODES__, REJECT, __PROXY_NODES__], include-all-proxies: true, filter: 'alpha|charlie', exclude-filter: charlie}\n  - {name: P, type: select, include-all-providers: true}\n  - {name: T, type: select, proxies: [__PROXY_PROVIDERS__]}\ndns: {nameserver: ['https://dns.example/__PROXY_NODES__#G']}\nrules: ['MATCH,G']\n";

#[tokio::test]
async fn placeholders_honor_fixed_selection_filters_positions_and_raw_history() {
    let db = TestDb::new(SPEC, "placeholders").await;
    let mut request = make_request(db.template_id, "mihomo");
    request.node_selection = Some(
        serde_json::from_value(json!({"mode":"fixed","nodeIds":[TROJAN_ID_A,TROJAN_ID_C]}))
            .expect("selector"),
    );
    let result = run(&db, request.clone(), "generate")
        .await
        .expect("expanded");
    let doc = deve_sub_yaml::parse_source_json(&result.content).expect("output");
    assert_eq!(doc["proxies"].as_array().expect("nodes").len(), 2);
    assert_eq!(
        doc["proxy-groups"][0]["proxies"],
        json!(["DIRECT", "alpha-node", "REJECT"])
    );
    for index in [1, 2] {
        assert_eq!(doc["proxy-groups"][index]["proxies"], json!(["REJECT"]));
    }
    assert_eq!(
        doc["dns"]["nameserver"][0],
        "https://dns.example/__PROXY_NODES__#G"
    );
    let stored: String = sqlx::query_scalar(
        "SELECT spec_yaml FROM template_versions WHERE template_id=? AND version=1",
    )
    .bind(db.template_id.to_string())
    .fetch_one(&db.pool)
    .await
    .expect("history");
    assert_eq!(stored, SPEC);
    let notices = result
        .warnings
        .iter()
        .filter(|w| w.contains("provider placeholder"))
        .collect::<Vec<_>>();
    assert_eq!(notices.len(), 2);
    for surface in ["generate", "preview", "delivery"] {
        let cached = run(&db, request.clone(), surface).await.expect("cache");
        assert_eq!(cached.content, result.content);
        for notice in &notices {
            assert!(cached.warnings.contains(notice));
        }
    }
}

#[tokio::test]
async fn placeholder_names_are_reserved_only_for_native_templates() {
    let id = "01KZAAAAAAAAAAAAAAAAAAAA03";
    for native in [false, true] {
        let spec = if native {
            "proxy-groups: [{name: G, type: select, proxies: [__PROXY_NODES__]}]\nrules: ['MATCH,G']"
        } else {
            SPEC_MIHOMO_ONLY
        };
        let db = TestDb::new(spec, "token-name").await;
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone())
            .import_nodes(vec![make_trojan(id, "__PROXY_NODES__", "token.example")])
            .await
            .expect("token named node");
        let mut request = make_request(db.template_id, "mihomo");
        request.node_selection = Some(
            serde_json::from_value(json!({"mode":"fixed","nodeIds":[id]})).expect("selection"),
        );
        let result = run(&db, request, "generate").await.expect("generate");
        let doc = deve_sub_yaml::parse_source_json(&result.content).expect("YAML");
        let expected = if native {
            format!("__PROXY_NODES__ [{id}]")
        } else {
            "__PROXY_NODES__".into()
        };
        assert_eq!(doc["proxies"][0]["name"], expected);
        if native {
            assert_eq!(doc["proxy-groups"][0]["proxies"], json!([expected]));
        }
    }
}

#[tokio::test]
async fn fallback_notices_use_historical_template_and_never_block_delivery() {
    let db = TestDb::new(SPEC, "fallback-notices").await;
    let request = make_request(db.template_id, "mihomo");
    let first = run(&db, request.clone(), "delivery")
        .await
        .expect("initial");
    update_template(&SqliteTemplateRepository::new(db.pool.clone()), UpdateTemplateParams {
        id: db.template_id, name: "fallback-notices".into(), description: "test".into(),
        spec_yaml: "proxy-groups: [{name: New, type: select, proxies: [missing, __PROXY_PROVIDERS__]}]\nrules: ['MATCH,New']".into(),
    }).await.expect("new version");
    let fallback = run(&db, request.clone(), "delivery")
        .await
        .expect("old content");
    assert_eq!(fallback.content, first.content);
    assert!(
        fallback
            .warnings
            .iter()
            .any(|w| w.contains("group 'P': external"))
    );
    assert!(!fallback.warnings.iter().any(|w| w.contains("group 'New'")));
    // Corrupt only inactive diagnostic metadata to exercise adapter read failure.
    sqlx::query(
        "UPDATE template_versions SET spec_json='invalid' WHERE template_id=? AND version=1",
    )
    .bind(db.template_id.to_string())
    .execute(&db.pool)
    .await
    .expect("inject read failure");
    let fallback = run(&db, request, "delivery")
        .await
        .expect("diagnostics must not break fallback");
    assert_eq!(fallback.content, first.content);
    assert!(
        fallback
            .warnings
            .iter()
            .any(|w| w == "template normalization warnings unavailable from cache")
    );
}
