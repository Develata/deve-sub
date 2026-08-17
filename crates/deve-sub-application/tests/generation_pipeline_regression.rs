//! DS-AUD-001 + DS-AUD-020 + DS-AUD-021 + DS-AUD-022 pipeline-level
//! regression tests.
//!
//! Verifies:
//! - generate() rejects a profile not declared in the template's
//!   targetProfiles (DS-AUD-001 #3: previously silently produced a
//!   proxy-only document, dropping groups/rules/dns/tun).
//! - generate() for mihomo emits a full document containing proxy-groups,
//!   rules, dns, and tun when the template spec declares them.
//! - Per-group sort_order (asc/desc) is applied to rendered members
//!   (DS-AUD-020).
//! - Cache-hit results honestly state "served from cache" and that the
//!   included/excluded report is unavailable from cache, rather than
//!   returning empty arrays that would imply "zero nodes" (DS-AUD-021).
//! - Group types incompatible with the target profile are rejected in
//!   strict mode and warned in lenient mode (DS-AUD-022).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use deve_sub_application::template::{CreateTemplateParams, create_template, generate};
use deve_sub_domain::{
    Authentication, DomainName, Endpoint, GenerationMode, GenerationRequest, Host, Node,
    NodePoolRepository, NodeSource, ProtocolConfig, ProtocolKind, RegionAssignment, RegionMethod,
    TrojanConfig, UdpCapability,
};
use deve_sub_kernel::Timestamp;
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{
    SqliteGenerationCacheRepository, SqliteNodePoolRepository, SqlitePoolMetaRepository,
    SqliteTemplateRepository, SqliteTemplateVersionRepository,
};

const TROJAN_ID_A: &str = "01KZAAAAAAAAAAAAAAAAAAAA00";
const TROJAN_ID_B: &str = "01KZAAAAAAAAAAAAAAAAAAAA01";
const TROJAN_ID_C: &str = "01KZAAAAAAAAAAAAAAAAAAAA02";

const SPEC_MIHOMO_ONLY: &str = concat!(
    "apiVersion: deve-sub.io/v1\n",
    "kind: SubscriptionTemplate\n",
    "\n",
    "metadata:\n",
    "  name: mihomo-only\n",
    "  description: mihomo-only template\n",
    "  version: 1\n",
    "\n",
    "spec:\n",
    "  targetProfiles:\n",
    "    - mihomo\n",
    "  variables: {}\n",
    "  nodeSelector:\n",
    "    mode: dynamic\n",
    "  proxyGroups: []\n",
    "  rules: []\n",
    "  dns: {}\n",
    "  tun: {}\n",
    "  output: {}",
);

const SPEC_FULL_TEMPLATE: &str = concat!(
    "apiVersion: deve-sub.io/v1\n",
    "kind: SubscriptionTemplate\n",
    "\n",
    "metadata:\n",
    "  name: full-mihomo\n",
    "  description: Full mihomo template with groups, rules, dns, tun\n",
    "  version: 1\n",
    "\n",
    "spec:\n",
    "  targetProfiles:\n",
    "    - mihomo\n",
    "  variables: {}\n",
    "  nodeSelector:\n",
    "    mode: dynamic\n",
    "  proxyGroups:\n",
    "    - name: select-all\n",
    "      type: select\n",
    "      members:\n",
    "        - kind: node\n",
    "          id: \"01KZAAAAAAAAAAAAAAAAAAAA00\"\n",
    "        - kind: node\n",
    "          id: \"01KZAAAAAAAAAAAAAAAAAAAA01\"\n",
    "        - kind: node\n",
    "          id: \"01KZAAAAAAAAAAAAAAAAAAAA02\"\n",
    "      sortOrder: asc\n",
    "  rules:\n",
    "    - value:\n",
    "        type: DOMAIN\n",
    "        domain: example.com\n",
    "        proxy: select-all\n",
    "  dns:\n",
    "    enable: true\n",
    "    nameserver:\n",
    "      - 8.8.8.8\n",
    "  tun:\n",
    "    enable: true\n",
    "    device: utun0\n",
    "  output: {}",
);

const SPEC_DESC_SORT: &str = concat!(
    "apiVersion: deve-sub.io/v1\n",
    "kind: SubscriptionTemplate\n",
    "\n",
    "metadata:\n",
    "  name: desc-sort\n",
    "  description: desc sort template\n",
    "  version: 1\n",
    "\n",
    "spec:\n",
    "  targetProfiles:\n",
    "    - mihomo\n",
    "  variables: {}\n",
    "  nodeSelector:\n",
    "    mode: dynamic\n",
    "  proxyGroups:\n",
    "    - name: select-all\n",
    "      type: select\n",
    "      members:\n",
    "        - kind: node\n",
    "          id: \"01KZAAAAAAAAAAAAAAAAAAAA00\"\n",
    "        - kind: node\n",
    "          id: \"01KZAAAAAAAAAAAAAAAAAAAA01\"\n",
    "        - kind: node\n",
    "          id: \"01KZAAAAAAAAAAAAAAAAAAAA02\"\n",
    "      sortOrder: desc\n",
    "  rules: []\n",
    "  dns: {}\n",
    "  tun: {}\n",
    "  output: {}",
);

struct TestDb {
    pool: sqlx::SqlitePool,
    master_key: Arc<MasterKey>,
    template_id: deve_sub_kernel::TemplateId,
    _dir: tempfile::TempDir,
}

impl TestDb {
    async fn new(spec_yaml: &str, name: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("test.db");
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
            .await
            .expect("pool");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("migrations");

        let master_key = Arc::new(MasterKey::from_bytes(&[0x42u8; 32]));
        let pool_repo =
            SqliteNodePoolRepository::new_with_key(pool.clone(), Arc::clone(&master_key));
        pool_repo
            .import_nodes(vec![
                make_trojan(TROJAN_ID_A, "alpha-node", "alpha.example.com"),
                make_trojan(TROJAN_ID_B, "bravo-node", "bravo.example.com"),
                make_trojan(TROJAN_ID_C, "charlie-node", "charlie.example.com"),
            ])
            .await
            .expect("import nodes");

        let template_repo = SqliteTemplateRepository::new(pool.clone());
        let version_repo = SqliteTemplateVersionRepository::new(pool.clone());
        let result = create_template(
            &template_repo,
            &version_repo,
            CreateTemplateParams {
                name: name.to_owned(),
                description: "test".to_owned(),
                spec_yaml: spec_yaml.to_owned(),
            },
        )
        .await
        .expect("create template");

        Self {
            pool,
            master_key,
            template_id: result.template.id,
            _dir: dir,
        }
    }
}

fn make_trojan(id: &str, name: &str, host: &str) -> Node {
    Node {
        id: deve_sub_kernel::NodeId::parse(id).expect("ulid"),
        display_name: name.to_owned(),
        protocol: ProtocolKind::Trojan,
        config: ProtocolConfig::Trojan(TrojanConfig {
            packet_encoding: None,
        }),
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new(host.to_owned())),
            port: 443,
        },
        authentication: Authentication::Password {
            password: "TEST_PASSWORD".to_owned(),
        },
        transport: None,
        tls: None,
        udp: UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "test".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::from_unix_ms(0).expect("ts"),
        },
        tags: vec![],
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: None,
        },
        extras: BTreeMap::new(),
    }
}

fn make_request(template_id: deve_sub_kernel::TemplateId, profile: &str) -> GenerationRequest {
    GenerationRequest::new(template_id, profile.to_owned(), GenerationMode::Lenient)
}

#[tokio::test]
async fn generate_rejects_profile_not_in_target_profiles() {
    let db = TestDb::new(SPEC_MIHOMO_ONLY, "mihomo-only").await;
    let template_repo = SqliteTemplateRepository::new(db.pool.clone());
    let version_repo = SqliteTemplateVersionRepository::new(db.pool.clone());
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let cache_repo = SqliteGenerationCacheRepository::new(db.pool.clone());
    let pool_meta_repo = SqlitePoolMetaRepository::new(db.pool.clone());

    // sing-box is not in targetProfiles (which declares only mihomo).
    let request = make_request(db.template_id, "sing-box");
    let result = generate(
        &template_repo,
        &version_repo,
        &pool_repo,
        &cache_repo,
        &pool_meta_repo,
        request,
    )
    .await;

    assert!(
        result.is_err(),
        "generate must reject a profile not in targetProfiles"
    );
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("target_profiles") || msg.contains("not in"),
        "error must mention target_profiles, got: {msg}"
    );
}

#[tokio::test]
async fn generate_mihomo_emits_full_template_with_groups_rules_dns_tun() {
    let db = TestDb::new(SPEC_FULL_TEMPLATE, "full-mihomo").await;
    let template_repo = SqliteTemplateRepository::new(db.pool.clone());
    let version_repo = SqliteTemplateVersionRepository::new(db.pool.clone());
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let cache_repo = SqliteGenerationCacheRepository::new(db.pool.clone());
    let pool_meta_repo = SqlitePoolMetaRepository::new(db.pool.clone());

    let request = make_request(db.template_id, "mihomo");
    let result = generate(
        &template_repo,
        &version_repo,
        &pool_repo,
        &cache_repo,
        &pool_meta_repo,
        request,
    )
    .await
    .expect("generate");

    let content = &result.content;
    assert!(content.contains("proxies:"), "must contain proxies");
    assert!(
        content.contains("proxy-groups:"),
        "must contain proxy-groups (DS-AUD-001 fix)"
    );
    assert!(content.contains("select-all"), "group name must appear");
    assert!(
        content.contains("rules:"),
        "must contain rules (DS-AUD-001 fix)"
    );
    assert!(content.contains("example.com"), "rule content must appear");
    assert!(
        content.contains("dns:"),
        "must contain dns (DS-AUD-001 fix)"
    );
    assert!(content.contains("8.8.8.8"), "dns content must appear");
    assert!(
        content.contains("tun:"),
        "must contain tun (DS-AUD-001 fix)"
    );
    assert!(content.contains("utun0"), "tun content must appear");
}

#[tokio::test]
async fn generate_applies_asc_sort_order_to_group_members() {
    let db = TestDb::new(SPEC_FULL_TEMPLATE, "asc-sort").await;
    let template_repo = SqliteTemplateRepository::new(db.pool.clone());
    let version_repo = SqliteTemplateVersionRepository::new(db.pool.clone());
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let cache_repo = SqliteGenerationCacheRepository::new(db.pool.clone());
    let pool_meta_repo = SqlitePoolMetaRepository::new(db.pool.clone());

    let request = make_request(db.template_id, "mihomo");
    let result = generate(
        &template_repo,
        &version_repo,
        &pool_repo,
        &cache_repo,
        &pool_meta_repo,
        request,
    )
    .await
    .expect("generate");

    let content = &result.content;
    // WHY: display names appear in both the `proxies:` section (sorted by
    // endpoint) and the `proxy-groups:` section (sorted by sortOrder). We
    // must check member order within the proxy-groups section only.
    let groups_section = content
        .split("proxy-groups:")
        .nth(1)
        .expect("proxy-groups section present");
    let alpha_pos = groups_section.find("alpha-node").expect("alpha present");
    let bravo_pos = groups_section.find("bravo-node").expect("bravo present");
    let charlie_pos = groups_section
        .find("charlie-node")
        .expect("charlie present");
    assert!(
        alpha_pos < bravo_pos && bravo_pos < charlie_pos,
        "sortOrder: asc must order members alphabetically (DS-AUD-020)"
    );
}

#[tokio::test]
async fn generate_applies_desc_sort_order_to_group_members() {
    let db = TestDb::new(SPEC_DESC_SORT, "desc-sort").await;
    let template_repo = SqliteTemplateRepository::new(db.pool.clone());
    let version_repo = SqliteTemplateVersionRepository::new(db.pool.clone());
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let cache_repo = SqliteGenerationCacheRepository::new(db.pool.clone());
    let pool_meta_repo = SqlitePoolMetaRepository::new(db.pool.clone());

    let request = make_request(db.template_id, "mihomo");
    let result = generate(
        &template_repo,
        &version_repo,
        &pool_repo,
        &cache_repo,
        &pool_meta_repo,
        request,
    )
    .await
    .expect("generate");

    let content = &result.content;
    let groups_section = content
        .split("proxy-groups:")
        .nth(1)
        .expect("proxy-groups section present");
    let alpha_pos = groups_section.find("alpha-node").expect("alpha present");
    let bravo_pos = groups_section.find("bravo-node").expect("bravo present");
    let charlie_pos = groups_section
        .find("charlie-node")
        .expect("charlie present");
    assert!(
        charlie_pos < bravo_pos && bravo_pos < alpha_pos,
        "sortOrder: desc must order members reverse-alphabetically (DS-AUD-020)"
    );
}

// WHY: xray's capability matrix only supports GroupType::Select. A url-test
// group is incompatible and must be caught before emission.
const SPEC_XRAY_URL_TEST_GROUP: &str = concat!(
    "apiVersion: deve-sub.io/v1\n",
    "kind: SubscriptionTemplate\n",
    "\n",
    "metadata:\n",
    "  name: xray-url-test\n",
    "  description: xray template with url-test group\n",
    "  version: 1\n",
    "\n",
    "spec:\n",
    "  targetProfiles:\n",
    "    - xray\n",
    "  variables: {}\n",
    "  nodeSelector:\n",
    "    mode: dynamic\n",
    "  proxyGroups:\n",
    "    - name: auto-select\n",
    "      type: url-test\n",
    "      members:\n",
    "        - kind: node\n",
    "          id: \"01KZAAAAAAAAAAAAAAAAAAAA00\"\n",
    "      sortOrder: asc\n",
    "  rules: []\n",
    "  dns: {}\n",
    "  tun: {}\n",
    "  output: {}",
);

#[tokio::test]
async fn generate_strict_mode_rejects_incompatible_group_type() {
    let db = TestDb::new(SPEC_XRAY_URL_TEST_GROUP, "xray-url-test").await;
    let template_repo = SqliteTemplateRepository::new(db.pool.clone());
    let version_repo = SqliteTemplateVersionRepository::new(db.pool.clone());
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let cache_repo = SqliteGenerationCacheRepository::new(db.pool.clone());
    let pool_meta_repo = SqlitePoolMetaRepository::new(db.pool.clone());

    let request = GenerationRequest::new(db.template_id, "xray".to_owned(), GenerationMode::Strict);
    let result = generate(
        &template_repo,
        &version_repo,
        &pool_repo,
        &cache_repo,
        &pool_meta_repo,
        request,
    )
    .await;

    assert!(
        result.is_err(),
        "strict mode must reject incompatible group type (DS-AUD-022)"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("incompatible group type"),
        "error must mention incompatible group type, got: {msg}"
    );
}

#[tokio::test]
async fn generate_lenient_mode_warns_on_incompatible_group_type() {
    let db = TestDb::new(SPEC_XRAY_URL_TEST_GROUP, "xray-url-test-lenient").await;
    let template_repo = SqliteTemplateRepository::new(db.pool.clone());
    let version_repo = SqliteTemplateVersionRepository::new(db.pool.clone());
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let cache_repo = SqliteGenerationCacheRepository::new(db.pool.clone());
    let pool_meta_repo = SqlitePoolMetaRepository::new(db.pool.clone());

    let request = make_request(db.template_id, "xray");
    let result = generate(
        &template_repo,
        &version_repo,
        &pool_repo,
        &cache_repo,
        &pool_meta_repo,
        request,
    )
    .await
    .expect("lenient mode must not error on incompatible group type");

    let incompatible_warning = result
        .warnings
        .iter()
        .find(|w| w.contains("incompatible") && w.contains("auto-select"));
    assert!(
        incompatible_warning.is_some(),
        "lenient mode must warn about incompatible group 'auto-select', got warnings: {:?}",
        result.warnings
    );
}

#[tokio::test]
async fn generate_cache_hit_returns_honest_cache_warnings() {
    let db = TestDb::new(SPEC_FULL_TEMPLATE, "cache-hit").await;
    let template_repo = SqliteTemplateRepository::new(db.pool.clone());
    let version_repo = SqliteTemplateVersionRepository::new(db.pool.clone());
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let cache_repo = SqliteGenerationCacheRepository::new(db.pool.clone());
    let pool_meta_repo = SqlitePoolMetaRepository::new(db.pool.clone());

    let request = make_request(db.template_id, "mihomo");

    let first = generate(
        &template_repo,
        &version_repo,
        &pool_repo,
        &cache_repo,
        &pool_meta_repo,
        request.clone(),
    )
    .await
    .expect("first generate");

    assert!(
        !first.warnings.iter().any(|w| w == "served from cache"),
        "first call must not be a cache hit"
    );

    let second = generate(
        &template_repo,
        &version_repo,
        &pool_repo,
        &cache_repo,
        &pool_meta_repo,
        request,
    )
    .await
    .expect("second generate (cache hit)");

    assert!(
        second.warnings.iter().any(|w| w == "served from cache"),
        "cache hit must honestly state 'served from cache' (DS-AUD-021), got warnings: {:?}",
        second.warnings
    );
    assert!(
        second
            .warnings
            .iter()
            .any(|w| w == "included/excluded report unavailable from cache"),
        "cache hit must state report is unavailable from cache (DS-AUD-021), got warnings: {:?}",
        second.warnings
    );
    assert!(
        second.included_node_ids.is_empty() && second.excluded.is_empty(),
        "cache hit must not fabricate included/excluded data (DS-AUD-021)"
    );
    assert_eq!(
        first.content, second.content,
        "cache hit must return identical content"
    );
}
