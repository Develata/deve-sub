//! GEN-015: withdrawn branches block locally while valid branches keep updating.
use super::*;
use deve_sub_domain::{
    ItemParseStatus, ReconcileEntry, ReconcileInput, Source, SourceRepository, SourceSnapshot,
    SourceType,
};
use deve_sub_kernel::{SourceId, SourceSnapshotId};
use deve_sub_storage_sqlite::SqliteSourceRepository;

const REMOTE_ID: &str = "01KZAAAAAAAAAAAAAAAAAAAA09";

async fn fixture(spec: &str) -> (TestDb, SourceId) {
    fixture_named(spec, "remote-A").await
}

async fn fixture_named(spec: &str, name: &str) -> (TestDb, SourceId) {
    let db = TestDb::new(spec, "withdrawal-groups").await;
    let sources = SqliteSourceRepository::new_with_key(db.pool.clone(), db.master_key.clone());
    let source = Source::new(
        "remote",
        SourceType::UriList,
        "https://source.example.com/sub".into(),
    );
    sources.create(&source).await.expect("source");
    let mut node = deve_sub_protocol::parse_uri("trojan://fixture@remote.example.com:443#remote-A")
        .expect("node");
    node.id = NodeId::parse(REMOTE_ID).expect("id");
    node.display_name = name.into();
    SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone())
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
    (db, source.id)
}

#[tokio::test]
async fn gen015_native_retained_node_alias_survives_withdrawn_name_collision() {
    for secondary_collision in [false, true] {
        let base_alias = format!("alpha-node [{TROJAN_ID_A}]");
        let alias = if secondary_collision {
            format!("{base_alias} 2")
        } else {
            base_alias.clone()
        };
        let extra = if secondary_collision {
            format!(", {{name: Exact, type: select, proxies: ['{base_alias}']}}")
        } else {
            String::new()
        };
        let spec = format!(
            "proxy-groups: [{{name: Retained, type: select, proxies: ['{alias}']}}{extra}]\nrules: ['MATCH,Retained']"
        );
        let (db, source) = fixture_named(&spec, "alpha-node").await;
        if secondary_collision {
            let mut node =
                deve_sub_protocol::parse_uri("trojan://fixture@name-owner.example.com:443#owner")
                    .expect("name owner");
            node.display_name = base_alias.clone();
            SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone())
                .import_nodes(vec![node])
                .await
                .expect("colliding alias");
        }
        run(&db, make_request(db.template_id, "mihomo"), "generate")
            .await
            .expect("duplicate names before deletion");
        withdraw(&db, source).await;
        let result = run(&db, make_request(db.template_id, "mihomo"), "generate")
            .await
            .expect("retained manual node");
        let output: serde_yaml::Value = serde_yaml::from_str(&result.content).expect("yaml");
        assert_eq!(
            group(&output, "Retained")["proxies"],
            serde_yaml::to_value(["alpha-node"]).expect("members")
        );
        assert!(!result.content.contains("remote.example.com"));
        if secondary_collision {
            assert_eq!(
                group(&output, "Exact")["proxies"],
                serde_yaml::to_value([base_alias]).expect("exact name wins over aliases")
            );
        }
    }
}

async fn withdraw(db: &TestDb, source: SourceId) {
    SqliteSourceRepository::new_with_key(db.pool.clone(), db.master_key.clone())
        .delete(source)
        .await
        .expect("delete");
}

#[tokio::test]
async fn gen015_native_withdrawn_secondary_alias_is_removed_without_losing_survivors() {
    let alias = format!("alpha-node [{REMOTE_ID}]");
    let spec = format!(
        "proxy-groups: [{{name: Mixed, type: select, proxies: ['{alias} 2', bravo-node]}}]\nrules: ['MATCH,Mixed']"
    );
    let (db, source) = fixture_named(&spec, "alpha-node").await;
    let mut owner =
        deve_sub_protocol::parse_uri("trojan://fixture@name-owner.example.com:443#owner")
            .expect("owner");
    owner.display_name = alias;
    SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone())
        .import_nodes(vec![owner])
        .await
        .expect("name owner");
    run(&db, make_request(db.template_id, "mihomo"), "generate")
        .await
        .expect("before");
    withdraw(&db, source).await;
    let result = run(&db, make_request(db.template_id, "mihomo"), "generate")
        .await
        .expect("withdrawn alias removed");
    let output: serde_yaml::Value = serde_yaml::from_str(&result.content).expect("yaml");
    assert_eq!(
        group(&output, "Mixed")["proxies"],
        serde_yaml::to_value(["bravo-node"]).expect("members")
    );
    assert!(!result.content.contains("remote.example.com"));
}

fn group<'a>(output: &'a serde_yaml::Value, name: &str) -> &'a serde_yaml::Value {
    output["proxy-groups"]
        .as_sequence()
        .expect("groups")
        .iter()
        .find(|g| g["name"].as_str() == Some(name))
        .expect("group")
}

#[tokio::test]
async fn gen015_withdrawn_v3_leaf_blocks_without_breaking_parent() {
    let spec = document(
        json!({"mode":"dynamic"}),
        json!([
            {"name":"Remote", "type":"select", "members":[{"kind":"node","id":REMOTE_ID}]},
            {"name":"Main", "type":"select", "members":[{"kind":"group","name":"Remote"},{"kind":"node","id":TROJAN_ID_A}]}
        ]),
    );
    let (db, source) = fixture(&spec).await;
    run(&db, make_request(db.template_id, "mihomo"), "generate")
        .await
        .expect("before");
    withdraw(&db, source).await;
    for mode in [GenerationMode::Lenient, GenerationMode::Strict] {
        let mut request = make_request(db.template_id, "mihomo");
        request.mode = mode;
        let result = run(&db, request, "generate")
            .await
            .expect("remaining branches");
        let output: serde_yaml::Value = serde_yaml::from_str(&result.content).expect("yaml");
        assert!(!result.content.contains("remote.example.com"));
        assert_eq!(group(&output, "Remote")["type"].as_str(), Some("select"));
        assert_eq!(
            group(&output, "Remote")["proxies"],
            serde_yaml::to_value(["REJECT"]).expect("members")
        );
        assert_eq!(
            group(&output, "Main")["proxies"],
            serde_yaml::to_value(["Remote", "alpha-node"]).expect("members")
        );
        assert!(result.warnings.iter().any(|w| w.contains("REJECT")));
    }
}

#[tokio::test]
async fn gen015_native_withdrawn_members_preserve_routes_and_options() {
    let spec = "proxy-groups:\n  - {name: Remote, type: url-test, proxies: [remote-A], url: 'https://example.com/check'}\n  - {name: Main, type: select, proxies: [Remote, alpha-node], hidden: true}\nrules: ['DOMAIN-SUFFIX,example.com,Remote', 'MATCH,Main']\ndns:\n  nameserver-policy:\n    'z.example.com': '1.1.1.1'\n    '+.example.com': '8.8.8.8'";
    let (db, source) = fixture(spec).await;
    run(&db, make_request(db.template_id, "mihomo"), "generate")
        .await
        .expect("before");
    withdraw(&db, source).await;
    let result = run(&db, make_request(db.template_id, "mihomo"), "delivery")
        .await
        .expect("fresh delivery");
    assert!(!result.content.contains("remote.example.com"));
    assert!(
        !result
            .warnings
            .iter()
            .any(|w| w.contains("last successful"))
    );
    let output: serde_yaml::Value = serde_yaml::from_str(&result.content).expect("yaml");
    assert_eq!(
        group(&output, "Remote")["proxies"],
        serde_yaml::to_value(["REJECT"]).expect("members")
    );
    assert_eq!(group(&output, "Main")["hidden"].as_bool(), Some(true));
    let policy: Vec<_> = output["dns"]["nameserver-policy"]
        .as_mapping()
        .expect("DNS policy")
        .keys()
        .map(|k| k.as_str().expect("key"))
        .collect();
    assert_eq!(
        policy,
        ["z.example.com", "+.example.com"],
        "first matching DNS policy retains author order"
    );
    assert_eq!(
        output["rules"],
        serde_yaml::to_value(["DOMAIN-SUFFIX,example.com,Remote", "MATCH,Main"]).expect("rules")
    );
}

#[tokio::test]
async fn gen015_native_exclusion_uses_client_adapter_types() {
    let spec = "proxy-groups:\n  - {name: Types, type: select, proxies: [DIRECT, COMPATIBLE, REJECT, REJECT-DROP, tuic], exclude-type: 'Tuic|Compatible|RejectDrop'}\nrules: ['MATCH,Types']";
    let db = TestDb::new(spec, "client-types").await;
    let node = deve_sub_protocol::parse_uri(
        "tuic://00000000-0000-4000-8000-000000000001:fixture@tuic.example.com:443#tuic",
    )
    .expect("TUIC");
    SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone())
        .import_nodes(vec![node])
        .await
        .expect("import");
    let result = run(&db, make_request(db.template_id, "mihomo"), "generate")
        .await
        .expect("generate");
    let output: serde_yaml::Value = serde_yaml::from_str(&result.content).expect("yaml");
    assert_eq!(
        group(&output, "Types")["proxies"],
        serde_yaml::to_value(["DIRECT", "REJECT"]).expect("members")
    );
}

#[tokio::test]
async fn gen015_native_auto_groups_cannot_fall_back_to_direct_when_empty() {
    for options in [
        "filter: '^remote-A$'",
        "filter: '^remote-A$', proxies: [REJECT], exclude-filter: 'REJECT'",
        "exclude-type: 'reject|trojan', proxies: [REJECT]",
    ] {
        let spec = format!(
            "proxy-groups: [{{name: Auto, type: url-test, include-all-proxies: true, {options}}}]\nrules: ['MATCH,Auto']"
        );
        let (db, source) = fixture(&spec).await;
        withdraw(&db, source).await;
        let result = run(&db, make_request(db.template_id, "mihomo"), "generate")
            .await
            .expect("safe empty group");
        let output: serde_yaml::Value = serde_yaml::from_str(&result.content).expect("yaml");
        let auto = group(&output, "Auto");
        assert_eq!(
            auto["proxies"],
            serde_yaml::to_value(["REJECT"]).expect("members")
        );
        assert_eq!(auto["type"].as_str(), Some("select"));
        for key in [
            "filter",
            "exclude-filter",
            "exclude-type",
            "include-all-proxies",
            "include-all",
        ] {
            assert!(
                auto.get(key).is_none(),
                "client must not reinterpret an already resolved group"
            );
        }
    }
}

#[tokio::test]
async fn gen015_native_auto_groups_resolve_lookaround_and_explicit_members() {
    let spec = "proxy-groups: [{name: Auto, type: select, proxies: [DIRECT, bravo-node], include-all: true, filter: '^(?!.*(?:bravo|charlie)).*', exclude-filter: '^DIRECT$'}]\nrules: ['MATCH,Auto']";
    let (db, source) = fixture(spec).await;
    withdraw(&db, source).await;
    let result = run(&db, make_request(db.template_id, "mihomo"), "generate")
        .await
        .expect("materialize");
    let output: serde_yaml::Value = serde_yaml::from_str(&result.content).expect("yaml");
    assert_eq!(
        group(&output, "Auto")["proxies"],
        serde_yaml::to_value(["bravo-node", "alpha-node"]).expect("members"),
        "inclusion filters apply to auto nodes; exclusion filters also apply to explicit members"
    );
}

#[tokio::test]
async fn gen015_native_unknown_names_and_invalid_filters_cannot_publish() {
    for options in [
        "proxies: [typo-node]".to_owned(),
        "include-all: true, filter: '['".to_owned(),
        format!("include-all: true, filter: '{}'", "a".repeat(4097)),
    ] {
        let spec = format!(
            "proxy-groups: [{{name: Auto, type: select, {options}}}]\nrules: ['MATCH,Auto']"
        );
        let (db, _) = fixture(&spec).await;
        assert!(
            run(&db, make_request(db.template_id, "mihomo"), "generate")
                .await
                .is_err()
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
