#![allow(clippy::expect_used)]

//! Acceptance tests for node override and GeoIP features (NODE-006~010).
//!
//! - NODE-006: manual region not overwritten by auto-detection
//! - NODE-007: auto region detection for IPv4
//! - NODE-008: auto region detection for IPv6
//! - NODE-009: dual-stack domain records both candidate IPs
//! - NODE-010: manual override survives upstream refresh

use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use deve_sub_application::source::{
    self, CreateSourceParams, FetchError, FetchResult, GeoIpPort, RegionDetection,
    SubscriptionFetcher, UpdateOverrideParams, enrich_regions,
};
use deve_sub_domain::{
    ItemParseStatus, NodePoolRepository, ReconcileEntry, RegionMethod, SourceType,
};
use deve_sub_kernel::{NodeId, TagId};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{
    SqliteNodeOverrideRepository, SqliteNodePoolRepository, SqliteSourceRepository,
    SqliteSourceSnapshotRepository,
};

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

struct TestDb {
    pool: sqlx::sqlite::SqlitePool,
    master_key: Arc<MasterKey>,
    _dir: tempfile::TempDir,
}

impl TestDb {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("test.db");
        let pool =
            sqlx::sqlite::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
                .await
                .expect("pool");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("migrations");
        Self {
            pool,
            master_key: Arc::new(MasterKey::from_bytes(&[0x42u8; 32])),
            _dir: dir,
        }
    }
}

/// Stub GeoIP that returns a fixed region and candidate IPs for any host.
struct TestGeoIp {
    region: Option<String>,
    ips: Vec<IpAddr>,
}

#[async_trait]
impl GeoIpPort for TestGeoIp {
    async fn detect_region(&self, _host: &str) -> RegionDetection {
        RegionDetection {
            region: self.region.clone(),
            candidate_ips: self.ips.clone(),
        }
    }
}

/// Mock fetcher returning a pre-programmed response.
struct MockFetcher {
    body: String,
}

#[async_trait]
impl SubscriptionFetcher for MockFetcher {
    async fn fetch(&self, _url: &str, _etag: Option<&str>) -> Result<FetchResult, FetchError> {
        Ok(FetchResult::Ok {
            body: self.body.as_bytes().to_vec(),
            etag: None,
            content_type: Some("text/plain".to_owned()),
        })
    }
}

const TROJAN_URI: &str = "trojan://TEST_PASSWORD@example.com:443?sni=example.com#NodeA";

/// Parse a URI string into ReconcileEntry values for enrich_regions tests.
fn parse_entries(uri: &str) -> Vec<ReconcileEntry> {
    let result =
        source::parse_for_import(SourceType::UriList, None, uri.as_bytes()).expect("parse");
    result
        .nodes
        .into_iter()
        .map(|n| ReconcileEntry {
            raw_uri: String::new(),
            initial_status: ItemParseStatus::Parsed,
            node: Some(n),
        })
        .collect()
}

/// Create a source and return it.
async fn create_source(repo: &SqliteSourceRepository, name: &str) -> deve_sub_domain::Source {
    source::create_source(
        repo,
        CreateSourceParams {
            name: name.to_owned(),
            source_type: SourceType::UriList,
            url: "https://example.com/sub".to_owned(),
            auto_update: false,
            update_interval_secs: 3600,
            keep_on_fail: true,
            filter_rules: None,
        },
    )
    .await
    .expect("create source")
}

// ---------------------------------------------------------------------------
// NODE-007: Auto region detection for IPv4
// ---------------------------------------------------------------------------

#[tokio::test]
async fn node_007_auto_region_ipv4() {
    let mut entries = parse_entries("trojan://pass@1.2.3.4:443#IPv4Node");
    let ipv4: IpAddr = "1.2.3.4".parse().expect("valid IPv4");
    let geoip = TestGeoIp {
        region: Some("US".to_owned()),
        ips: vec![ipv4],
    };
    enrich_regions(&mut entries, &geoip).await;
    let node = entries[0].node.as_ref().expect("parsed node");
    assert_eq!(node.region.method, RegionMethod::Auto);
    assert_eq!(node.region.value.as_deref(), Some("US"));
}

// ---------------------------------------------------------------------------
// NODE-008: Auto region detection for IPv6
// ---------------------------------------------------------------------------

#[tokio::test]
async fn node_008_auto_region_ipv6() {
    let mut entries = parse_entries("trojan://pass@[2001:db8::1]:443#IPv6Node");
    let ipv6: IpAddr = "2001:db8::1".parse().expect("valid IPv6");
    let geoip = TestGeoIp {
        region: Some("DE".to_owned()),
        ips: vec![ipv6],
    };
    enrich_regions(&mut entries, &geoip).await;
    let node = entries[0].node.as_ref().expect("parsed node");
    assert_eq!(node.region.method, RegionMethod::Auto);
    assert_eq!(node.region.value.as_deref(), Some("DE"));
}

// ---------------------------------------------------------------------------
// NODE-009: Dual-stack domain records both candidate IPs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn node_009_dual_stack_candidate_ips() {
    let mut entries = parse_entries("trojan://pass@example.com:443#DualStack");
    let ipv4: IpAddr = "1.2.3.4".parse().expect("valid IPv4");
    let ipv6: IpAddr = "2001:db8::1".parse().expect("valid IPv6");
    let geoip = TestGeoIp {
        region: Some("US".to_owned()),
        ips: vec![ipv4, ipv6],
    };
    enrich_regions(&mut entries, &geoip).await;
    let node = entries[0].node.as_ref().expect("parsed node");
    let candidate_ips = node
        .extras
        .get("candidate_ips")
        .expect("candidate_ips should be recorded");
    let ips: Vec<String> =
        serde_json::from_value(candidate_ips.clone()).expect("parse candidate_ips");
    assert!(
        ips.contains(&"1.2.3.4".to_owned()),
        "IPv4 candidate should be recorded"
    );
    assert!(
        ips.contains(&"2001:db8::1".to_owned()),
        "IPv6 candidate should be recorded"
    );
}

// ---------------------------------------------------------------------------
// NODE-006: Manual region not overwritten by auto-detection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn node_006_manual_region_survives_auto_detection() {
    let db = TestDb::new().await;
    let source_repo =
        SqliteSourceRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let override_repo = SqliteNodeOverrideRepository::new(db.pool.clone());
    let fetcher = MockFetcher {
        body: TROJAN_URI.to_owned(),
    };
    // Auto-detect "US" on refresh
    let geoip = TestGeoIp {
        region: Some("US".to_owned()),
        ips: vec![],
    };

    let src = create_source(&source_repo, "node-006-source").await;

    // First refresh: inserts node with auto-detected region "US"
    source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher,
        &geoip,
        src.id,
    )
    .await
    .expect("first refresh");

    let nodes = pool_repo
        .list_nodes(&Default::default(), None, 100)
        .await
        .expect("list nodes");
    assert_eq!(nodes.len(), 1);
    let node_id = nodes[0].node.id;

    // Set manual region override to "JP"
    let region =
        source::set_manual_region(&override_repo, &pool_repo, node_id, Some("JP".to_owned()))
            .await
            .expect("set manual region");
    assert_eq!(region.method, RegionMethod::Manual);
    assert_eq!(region.value.as_deref(), Some("JP"));

    // Second refresh: auto-detects "US" again, but manual override must persist
    source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher,
        &geoip,
        src.id,
    )
    .await
    .expect("second refresh");

    // Verify effective region is still "JP" (manual), not "US" (auto)
    let entry = pool_repo
        .get_node(node_id)
        .await
        .expect("get node")
        .expect("node exists");
    assert_eq!(entry.node.region.method, RegionMethod::Manual);
    assert_eq!(entry.node.region.value.as_deref(), Some("JP"));
}

// ---------------------------------------------------------------------------
// NODE-010: Manual override survives upstream refresh
// ---------------------------------------------------------------------------

#[tokio::test]
async fn node_010_override_survives_refresh() {
    let db = TestDb::new().await;
    let source_repo =
        SqliteSourceRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let override_repo = SqliteNodeOverrideRepository::new(db.pool.clone());
    let fetcher = MockFetcher {
        body: TROJAN_URI.to_owned(),
    };
    let geoip = TestGeoIp {
        region: None,
        ips: vec![],
    };

    let src = create_source(&source_repo, "node-010-source").await;

    // First refresh: inserts node
    source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher,
        &geoip,
        src.id,
    )
    .await
    .expect("first refresh");

    let nodes = pool_repo
        .list_nodes(&Default::default(), None, 100)
        .await
        .expect("list nodes");
    assert_eq!(nodes.len(), 1);
    let node_id = nodes[0].node.id;

    // Set override: custom display name and disabled
    source::update_override(
        &override_repo,
        &pool_repo,
        node_id,
        UpdateOverrideParams {
            display_name: Some("Custom Name".to_owned()),
            region: None,
            enabled: Some(false),
            sni: None,
            skip_cert_verify: None,
            fingerprint: None,
            sort_order: 0,
        },
    )
    .await
    .expect("set override");

    // Verify override is effective before refresh
    let entry = pool_repo
        .get_node(node_id)
        .await
        .expect("get node")
        .expect("node exists");
    assert_eq!(entry.node.display_name, "Custom Name");
    assert!(!entry.is_active);
    assert!(entry.override_info.is_some());

    // Second refresh: upstream sends the same node
    source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher,
        &geoip,
        src.id,
    )
    .await
    .expect("second refresh");

    // Verify override still effective after refresh
    let entry = pool_repo
        .get_node(node_id)
        .await
        .expect("get node")
        .expect("node exists");
    assert_eq!(
        entry.node.display_name, "Custom Name",
        "override display_name must survive refresh"
    );
    assert!(
        !entry.is_active,
        "override enabled=false must survive refresh"
    );
    assert!(entry.override_info.is_some(), "override must still exist");
}

// ---------------------------------------------------------------------------
// NODE-004: Batch enable/disable
// ---------------------------------------------------------------------------

const MULTI_NODE_URI: &str = "trojan://pass@host1.com:443#Node1\n\
     trojan://pass@host2.com:443#Node2\n\
     trojan://pass@host3.com:443#Node3";

#[tokio::test]
async fn node_004_batch_enable_disable() {
    let db = TestDb::new().await;
    let source_repo =
        SqliteSourceRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let override_repo = SqliteNodeOverrideRepository::new(db.pool.clone());
    let fetcher = MockFetcher {
        body: MULTI_NODE_URI.to_owned(),
    };
    let geoip = TestGeoIp {
        region: None,
        ips: vec![],
    };

    let src = create_source(&source_repo, "node-004-source").await;
    source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher,
        &geoip,
        src.id,
    )
    .await
    .expect("refresh");

    let nodes = pool_repo
        .list_nodes(&Default::default(), None, 100)
        .await
        .expect("list nodes");
    assert_eq!(nodes.len(), 3);
    let ids: Vec<NodeId> = nodes.iter().map(|n| n.node.id).collect();
    let batch = vec![ids[0], ids[1]];

    let count = source::batch_set_enabled(&override_repo, batch.clone(), false)
        .await
        .expect("batch disable");
    assert_eq!(count, 2);

    for id in &batch {
        let entry = pool_repo.get_node(*id).await.expect("get").expect("exists");
        assert!(!entry.is_active, "node {id} should be disabled");
    }
    let entry = pool_repo
        .get_node(ids[2])
        .await
        .expect("get")
        .expect("exists");
    assert!(entry.is_active, "third node should remain active");

    let count = source::batch_set_enabled(&override_repo, batch.clone(), true)
        .await
        .expect("batch enable");
    assert_eq!(count, 2);
    for id in &batch {
        let entry = pool_repo.get_node(*id).await.expect("get").expect("exists");
        assert!(entry.is_active, "node {id} should be re-enabled");
    }
}

// ---------------------------------------------------------------------------
// NODE-005: Batch tags
// ---------------------------------------------------------------------------

#[tokio::test]
async fn node_005_batch_tags() {
    let db = TestDb::new().await;
    let source_repo =
        SqliteSourceRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let override_repo = SqliteNodeOverrideRepository::new(db.pool.clone());
    let fetcher = MockFetcher {
        body: MULTI_NODE_URI.to_owned(),
    };
    let geoip = TestGeoIp {
        region: None,
        ips: vec![],
    };

    let src = create_source(&source_repo, "node-005-source").await;
    source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher,
        &geoip,
        src.id,
    )
    .await
    .expect("refresh");

    let nodes = pool_repo
        .list_nodes(&Default::default(), None, 100)
        .await
        .expect("list nodes");
    assert_eq!(nodes.len(), 3);
    let ids: Vec<NodeId> = nodes.iter().map(|n| n.node.id).collect();

    let tag1 = source::create_tag(&override_repo, "premium", Some("#ff0000"))
        .await
        .expect("create tag1");
    let tag2 = source::create_tag(&override_repo, "backup", None)
        .await
        .expect("create tag2");

    let assignments: Vec<(NodeId, Vec<TagId>)> =
        vec![(ids[0], vec![tag1.id]), (ids[1], vec![tag1.id, tag2.id])];
    source::batch_set_tags(&override_repo, assignments)
        .await
        .expect("batch set tags");

    // NODE-005 assertion: tag query reflects assignments immediately
    let nodes = pool_repo
        .list_nodes(&Default::default(), None, 100)
        .await
        .expect("list nodes");
    let n0 = nodes.iter().find(|n| n.node.id == ids[0]).expect("node0");
    assert_eq!(n0.tags.len(), 1);
    assert_eq!(n0.tags[0].name, "premium");

    let n1 = nodes.iter().find(|n| n.node.id == ids[1]).expect("node1");
    assert_eq!(n1.tags.len(), 2);
    let names: Vec<&str> = n1.tags.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"premium"));
    assert!(names.contains(&"backup"));

    // Node2 has no tags
    let n2 = nodes.iter().find(|n| n.node.id == ids[2]).expect("node2");
    assert!(n2.tags.is_empty());
}
