#![allow(clippy::expect_used)]

//! Integration tests for `SqliteNodeOverrideRepository` and the node pool
//! read-path LEFT JOIN to `node_overrides` / `node_tags`.
//!
//! Covers NODE-004 (enabled override), NODE-005 (tag assignment),
//! NODE-006 (manual region override), and NODE-010 (override survives
//! across source refreshes — the read path reconstructs it). See
//! `docs/plan/milestones/M4-sources-and-node-pool.md` Slice 4.

use std::sync::Arc;

use deve_sub_domain::{
    NodeFilter, NodeOverride, NodeOverrideRepository, NodePoolRepository, RegionMethod, SourceError,
};
use deve_sub_kernel::{NodeId, NodeOverrideId};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{SqliteNodeOverrideRepository, SqliteNodePoolRepository};

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

const TROJAN_A: &str = "trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#NodeA";
const TROJAN_B: &str = "trojan://TEST_PASSWORD@other.com:8443?sni=other.com&type=tcp#NodeB";

fn trojan_node(uri: &str) -> deve_sub_domain::Node {
    let mut node = deve_sub_protocol::parse_uri(uri).expect("parse trojan URI");
    node.id = NodeId::new();
    node.source.imported_at = deve_sub_kernel::Timestamp::now();
    node
}

async fn import_node(db: &TestDb) -> NodeId {
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let node = trojan_node(TROJAN_A);
    let id = node.id;
    pool_repo.import_nodes(vec![node]).await.expect("import");
    id
}

fn make_override(node_id: NodeId) -> NodeOverride {
    NodeOverride {
        id: NodeOverrideId::new(),
        node_id,
        display_name: Some("Override Name".to_owned()),
        region: Some("US".to_owned()),
        enabled: Some(false),
        sni: Some("sni.example.com".to_owned()),
        skip_cert_verify: Some(true),
        fingerprint: Some("fp123".to_owned()),
        sort_order: 42,
    }
}

/// Override CRUD round-trip: upsert, get, verify all fields, delete, get None.
#[tokio::test]
async fn override_crud_round_trip() {
    let db = TestDb::new().await;
    let node_id = import_node(&db).await;
    let ov_repo = SqliteNodeOverrideRepository::new(db.pool.clone());

    let ov = make_override(node_id);
    ov_repo.upsert_override(&ov).await.expect("upsert");

    let got = ov_repo
        .get_override(node_id)
        .await
        .expect("get")
        .expect("override exists");
    assert_eq!(got, ov, "all fields round-trip");

    ov_repo.delete_override(node_id).await.expect("delete");
    let after = ov_repo
        .get_override(node_id)
        .await
        .expect("get after delete");
    assert!(after.is_none(), "override gone after delete");
}

/// `patch_override_region` sets the region without clearing other fields.
#[tokio::test]
async fn patch_override_region_preserves_other_fields() {
    let db = TestDb::new().await;
    let node_id = import_node(&db).await;
    let ov_repo = SqliteNodeOverrideRepository::new(db.pool.clone());

    let ov = NodeOverride {
        id: NodeOverrideId::new(),
        node_id,
        display_name: Some("Original".to_owned()),
        region: Some("US".to_owned()),
        enabled: Some(false),
        sni: Some("sni.example.com".to_owned()),
        skip_cert_verify: None,
        fingerprint: Some("fp".to_owned()),
        sort_order: 7,
    };
    ov_repo.upsert_override(&ov).await.expect("upsert");

    ov_repo
        .patch_override_region(node_id, Some("JP".to_owned()))
        .await
        .expect("patch region");

    let got = ov_repo
        .get_override(node_id)
        .await
        .expect("get")
        .expect("override exists");
    assert_eq!(got.region, Some("JP".to_owned()), "region updated");
    assert_eq!(
        got.display_name,
        Some("Original".to_owned()),
        "display_name preserved"
    );
    assert_eq!(got.enabled, Some(false), "enabled preserved");
    assert_eq!(got.sni, Some("sni.example.com".to_owned()), "sni preserved");
    assert_eq!(
        got.fingerprint,
        Some("fp".to_owned()),
        "fingerprint preserved"
    );
    assert_eq!(got.sort_order, 7, "sort_order preserved");
}

/// `batch_set_enabled` upserts `enabled` without clearing other fields.
#[tokio::test]
async fn batch_set_enabled_preserves_other_fields() {
    let db = TestDb::new().await;
    let node_id = import_node(&db).await;
    let ov_repo = SqliteNodeOverrideRepository::new(db.pool.clone());

    let ov = NodeOverride {
        id: NodeOverrideId::new(),
        node_id,
        display_name: Some("Original".to_owned()),
        region: Some("US".to_owned()),
        enabled: Some(false),
        sni: None,
        skip_cert_verify: None,
        fingerprint: None,
        sort_order: 0,
    };
    ov_repo.upsert_override(&ov).await.expect("upsert");

    let count = ov_repo
        .batch_set_enabled(&[node_id], true)
        .await
        .expect("batch set enabled");
    assert_eq!(count, 1, "one row affected");

    let got = ov_repo
        .get_override(node_id)
        .await
        .expect("get")
        .expect("override exists");
    assert_eq!(got.enabled, Some(true), "enabled updated to true");
    assert_eq!(
        got.display_name,
        Some("Original".to_owned()),
        "display_name preserved"
    );
    assert_eq!(got.region, Some("US".to_owned()), "region preserved");
}

/// `set_node_tags` replaces the tag set: add tags, then clear, verify empty.
#[tokio::test]
async fn set_node_tags_replaces_tags() {
    let db = TestDb::new().await;
    let node_id = import_node(&db).await;
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let ov_repo = SqliteNodeOverrideRepository::new(db.pool.clone());

    let tag1 = ov_repo
        .create_tag("Tag1", Some("#ff0000"))
        .await
        .expect("create tag1");
    let tag2 = ov_repo.create_tag("Tag2", None).await.expect("create tag2");

    ov_repo
        .set_node_tags(node_id, &[tag1.id, tag2.id])
        .await
        .expect("set tags");

    let entries = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].tags.len(), 2, "two tags assigned");

    ov_repo
        .set_node_tags(node_id, &[])
        .await
        .expect("clear tags");
    let entries = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list after clear");
    assert!(entries[0].tags.is_empty(), "tags empty after clear");
}

/// `batch_set_tags` replaces tags for multiple nodes in one transaction.
#[tokio::test]
async fn batch_set_tags_for_multiple_nodes() {
    let db = TestDb::new().await;
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let ov_repo = SqliteNodeOverrideRepository::new(db.pool.clone());

    let node_a = trojan_node(TROJAN_A);
    let id_a = node_a.id;
    let node_b = trojan_node(TROJAN_B);
    let id_b = node_b.id;
    pool_repo
        .import_nodes(vec![node_a, node_b])
        .await
        .expect("import");

    let tag1 = ov_repo
        .create_tag("Alpha", Some("#ff0000"))
        .await
        .expect("create tag1");
    let tag2 = ov_repo.create_tag("Beta", None).await.expect("create tag2");

    ov_repo
        .batch_set_tags(&[(id_a, vec![tag1.id]), (id_b, vec![tag2.id])])
        .await
        .expect("batch set tags");

    let entries = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list");
    assert_eq!(entries.len(), 2);
    for entry in &entries {
        assert_eq!(entry.tags.len(), 1, "each node has one tag");
    }
}

/// `create_tag` + `list_tags` + `delete_tag` with cascade and error cases.
#[tokio::test]
async fn tag_crud_and_cascade() {
    let db = TestDb::new().await;
    let node_id = import_node(&db).await;
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let ov_repo = SqliteNodeOverrideRepository::new(db.pool.clone());

    let tag1 = ov_repo
        .create_tag("Alpha", Some("#ff0000"))
        .await
        .expect("create tag1");
    let _tag2 = ov_repo.create_tag("Beta", None).await.expect("create tag2");

    let tags = ov_repo.list_tags().await.expect("list");
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].name, "Alpha", "ordered by name");
    assert_eq!(tags[1].name, "Beta");
    assert_eq!(tags[0].color.as_deref(), Some("#ff0000"));

    ov_repo
        .set_node_tags(node_id, &[tag1.id])
        .await
        .expect("assign tag");
    let entries = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list");
    assert_eq!(entries[0].tags.len(), 1);
    assert_eq!(entries[0].tags[0].name, "Alpha");

    ov_repo.delete_tag(tag1.id).await.expect("delete tag");

    let tags = ov_repo.list_tags().await.expect("list after delete");
    assert_eq!(tags.len(), 1, "one tag remaining");
    assert_eq!(tags[0].name, "Beta");

    let entries = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list after cascade");
    assert!(
        entries[0].tags.is_empty(),
        "tag cascaded from node_tags via FK ON DELETE CASCADE"
    );

    assert!(
        matches!(
            ov_repo.delete_tag(tag1.id).await,
            Err(SourceError::TagNotFound)
        ),
        "deleting non-existent tag returns TagNotFound"
    );

    assert!(
        matches!(
            ov_repo.create_tag("Beta", None).await,
            Err(SourceError::TagExists)
        ),
        "duplicate tag name returns TagExists"
    );
}

/// Read path: override affects effective `display_name`, `region.method`,
/// and `is_active` when `override.enabled = Some(false)`.
#[tokio::test]
async fn list_nodes_applies_override() {
    let db = TestDb::new().await;
    let node_id = import_node(&db).await;
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let ov_repo = SqliteNodeOverrideRepository::new(db.pool.clone());

    let ov = NodeOverride {
        id: NodeOverrideId::new(),
        node_id,
        display_name: Some("Custom Name".to_owned()),
        region: Some("HK".to_owned()),
        enabled: Some(false),
        sni: None,
        skip_cert_verify: None,
        fingerprint: None,
        sort_order: 0,
    };
    ov_repo.upsert_override(&ov).await.expect("upsert");

    let entries = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];

    assert_eq!(
        entry.node.display_name, "Custom Name",
        "effective display_name from override"
    );
    assert_eq!(
        entry.node.region.method,
        RegionMethod::Manual,
        "region method Manual when override region set"
    );
    assert_eq!(
        entry.node.region.value.as_deref(),
        Some("HK"),
        "effective region from override"
    );
    assert!(!entry.is_active, "override enabled=false forces inactive");
    assert!(entry.override_info.is_some(), "override_info populated");
    assert_eq!(entry.override_info, Some(ov));
}

/// Read path: tags are returned with names and colors via the tags subquery.
#[tokio::test]
async fn list_nodes_returns_tags() {
    let db = TestDb::new().await;
    let node_id = import_node(&db).await;
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let ov_repo = SqliteNodeOverrideRepository::new(db.pool.clone());

    let tag1 = ov_repo
        .create_tag("Production", Some("#00ff00"))
        .await
        .expect("create tag1");
    let _tag2 = ov_repo
        .create_tag("Staging", None)
        .await
        .expect("create tag2");

    ov_repo
        .set_node_tags(node_id, &[tag1.id])
        .await
        .expect("assign tag");

    let entries = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.tags.len(), 1, "one tag assigned");
    assert_eq!(entry.tags[0].name, "Production");
    assert_eq!(entry.tags[0].color.as_deref(), Some("#00ff00"));

    let got = pool_repo
        .get_node(node_id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(got.tags.len(), 1, "get_node also returns tags");
    assert_eq!(got.tags[0].name, "Production");
}
