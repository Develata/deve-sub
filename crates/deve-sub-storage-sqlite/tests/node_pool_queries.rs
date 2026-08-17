#![allow(clippy::expect_used)]

//! Integration tests for `SqliteNodePoolRepository` query and import methods.
//!
//! Covers NODE-001 (manual import), NODE-003 (dedup does not overwrite
//! different-credential nodes), and NODE-011 (missing-from-source flag is
//! preserved across queries). Also verifies `list_nodes` filtering and
//! `get_node` round-trip. See
//! `docs/plan/milestones/M4-sources-and-node-pool.md` Slice 3.

use deve_sub_domain::{
    ImportOutcome, ItemParseStatus, NodeFilter, NodePoolRepository, ProtocolKind, ReconcileEntry,
    ReconcileInput, Source, SourceRepository, SourceSnapshot, SourceType,
};
use deve_sub_kernel::{NodeId, SourceId, SourceSnapshotId, Timestamp};
use deve_sub_storage_sqlite::{SqliteNodePoolRepository, SqliteSourceRepository};

struct TestDb {
    pool: sqlx::sqlite::SqlitePool,
    master_key: std::sync::Arc<deve_sub_security::MasterKey>,
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
            master_key: std::sync::Arc::new(deve_sub_security::MasterKey::from_bytes(
                &[0x42u8; 32],
            )),
            _dir: dir,
        }
    }
}

fn make_source(name: &str) -> Source {
    let mut s = Source::new(
        name,
        SourceType::UriList,
        "https://example.com/sub".to_owned(),
    );
    s.id = SourceId::new();
    s
}

fn make_snapshot(source_id: SourceId, version: u64, node_count: u64) -> SourceSnapshot {
    SourceSnapshot {
        id: SourceSnapshotId::new(),
        source_id,
        version,
        fetched_at: Timestamp::now(),
        etag: None,
        node_count,
        is_active: true,
    }
}

fn trojan_node(uri: &str) -> deve_sub_domain::Node {
    let mut node = deve_sub_protocol::parse_uri(uri).expect("parse trojan URI");
    node.id = NodeId::new();
    node.source.imported_at = Timestamp::now();
    node
}

fn entry(node: deve_sub_domain::Node) -> ReconcileEntry {
    let raw = node.source.raw_uri.clone().unwrap_or_default();
    ReconcileEntry {
        raw_uri: raw,
        initial_status: ItemParseStatus::Parsed,
        node: Some(node),
    }
}

const TROJAN_A: &str = "trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#NodeA";
const TROJAN_B: &str = "trojan://TEST_PASSWORD@other.com:8443?sni=other.com&type=tcp#NodeB";
const TROJAN_A_DUP: &str =
    "trojan://DIFFERENT_PASSWORD@example.com:443?sni=example.com&type=tcp#NodeADup";

/// NODE-001: Manual import inserts new nodes into the pool.
#[tokio::test]
async fn import_inserts_new_nodes() {
    let db = TestDb::new().await;
    let pool_repo = SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );

    let nodes = vec![trojan_node(TROJAN_A), trojan_node(TROJAN_B)];
    let result = pool_repo.import_nodes(nodes).await.expect("import");

    assert_eq!(result.new_nodes, 2);
    assert_eq!(result.duplicate_nodes, 0);
    assert_eq!(result.failed, 0);
    assert_eq!(result.outcomes.len(), 2);
    assert!(matches!(result.outcomes[0], ImportOutcome::Inserted(_)));
    assert!(matches!(result.outcomes[1], ImportOutcome::Inserted(_)));

    let entries = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list");
    assert_eq!(entries.len(), 2);
}

/// NODE-003: A duplicate node (same protocol+host+port) does not overwrite
/// the existing node's credentials.
#[tokio::test]
async fn import_duplicate_does_not_overwrite_credentials() {
    let db = TestDb::new().await;
    let pool_repo = SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );

    let node_a = trojan_node(TROJAN_A);
    let original_id = node_a.id;
    pool_repo
        .import_nodes(vec![node_a])
        .await
        .expect("import first");

    // Second import with a DIFFERENT password but same endpoint.
    let node_dup = trojan_node(TROJAN_A_DUP);
    let result = pool_repo
        .import_nodes(vec![node_dup])
        .await
        .expect("import dup");

    assert_eq!(result.new_nodes, 0);
    assert_eq!(result.duplicate_nodes, 1);
    assert!(matches!(result.outcomes[0], ImportOutcome::Duplicate(id) if id == original_id));

    let entries = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list");
    assert_eq!(entries.len(), 1, "duplicate should not create a second row");
    assert_eq!(entries[0].node.id, original_id, "original node preserved");
}

/// NODE-011: A node marked missing via reconcile is reactivated when
/// manually imported with the same endpoint.
#[tokio::test]
async fn import_reactivates_missing_node() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );
    let pool_repo = SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );

    let source = make_source("test-source");
    source_repo.create(&source).await.expect("create source");

    // v1: two nodes.
    let entries_v1 = [entry(trojan_node(TROJAN_A)), entry(trojan_node(TROJAN_B))];
    let snap_v1 = make_snapshot(source.id, 1, 2);
    pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snap_v1,
            entries: &entries_v1,
        })
        .await
        .expect("reconcile v1");

    // v2: only TROJAN_A → TROJAN_B becomes missing.
    let entries_v2 = [entry(trojan_node(TROJAN_A))];
    let snap_v2 = make_snapshot(source.id, 2, 1);
    pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snap_v2,
            entries: &entries_v2,
        })
        .await
        .expect("reconcile v2");

    // Confirm TROJAN_B is missing.
    let active_filter = NodeFilter::active_only();
    let active = pool_repo
        .list_nodes(&active_filter, None, 100)
        .await
        .expect("list active");
    assert_eq!(active.len(), 1, "only TROJAN_A should be active");

    // Manually import TROJAN_B's endpoint → should reactivate, not insert.
    let result = pool_repo
        .import_nodes(vec![trojan_node(TROJAN_B)])
        .await
        .expect("import");
    assert_eq!(result.new_nodes, 1, "reactivation counts as new");
    assert_eq!(result.duplicate_nodes, 0);

    let active_after = pool_repo
        .list_nodes(&active_filter, None, 100)
        .await
        .expect("list active after");
    assert_eq!(active_after.len(), 2, "TROJAN_B reactivated");
}

/// `get_node` returns the full pool entry with metadata.
#[tokio::test]
async fn get_node_returns_pool_metadata() {
    let db = TestDb::new().await;
    let pool_repo = SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );

    let node = trojan_node(TROJAN_A);
    let id = node.id;
    pool_repo.import_nodes(vec![node]).await.expect("import");

    let entry = pool_repo
        .get_node(id)
        .await
        .expect("get")
        .expect("node exists");
    assert_eq!(entry.node.id, id);
    assert!(!entry.missing_from_source);
    assert!(entry.is_active);
    assert_eq!(entry.revision, 0);
    assert_eq!(entry.node.protocol, ProtocolKind::Trojan);
    assert_eq!(entry.node.endpoint.host.uri_host(), "example.com");
    assert_eq!(entry.node.endpoint.port, 443);
}

/// `get_node` returns None for an unknown ID.
#[tokio::test]
async fn get_node_returns_none_for_unknown() {
    let db = TestDb::new().await;
    let pool_repo = SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );

    let entry = pool_repo.get_node(NodeId::new()).await.expect("get");
    assert!(entry.is_none());
}

/// `list_nodes` with protocol filter returns only matching nodes.
#[tokio::test]
async fn list_nodes_filters_by_protocol() {
    let db = TestDb::new().await;
    let pool_repo = SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );

    pool_repo
        .import_nodes(vec![trojan_node(TROJAN_A), trojan_node(TROJAN_B)])
        .await
        .expect("import");

    let filter = NodeFilter {
        protocol: Some(ProtocolKind::Shadowsocks),
        region: None,
        include_missing: false,
        include_inactive: false,
    };
    let entries = pool_repo
        .list_nodes(&filter, None, 100)
        .await
        .expect("list");
    assert!(entries.is_empty(), "no Shadowsocks nodes");

    let filter_trojan = NodeFilter {
        protocol: Some(ProtocolKind::Trojan),
        region: None,
        include_missing: false,
        include_inactive: false,
    };
    let entries = pool_repo
        .list_nodes(&filter_trojan, None, 100)
        .await
        .expect("list trojan");
    assert_eq!(entries.len(), 2);
}

/// `list_nodes` with the active-only filter excludes missing nodes.
#[tokio::test]
async fn list_nodes_active_only_excludes_missing() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );
    let pool_repo = SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );

    let source = make_source("test-source");
    source_repo.create(&source).await.expect("create source");

    let entries_v1 = [entry(trojan_node(TROJAN_A)), entry(trojan_node(TROJAN_B))];
    let snap_v1 = make_snapshot(source.id, 1, 2);
    pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snap_v1,
            entries: &entries_v1,
        })
        .await
        .expect("reconcile v1");

    let entries_v2 = [entry(trojan_node(TROJAN_A))];
    let snap_v2 = make_snapshot(source.id, 2, 1);
    pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snap_v2,
            entries: &entries_v2,
        })
        .await
        .expect("reconcile v2");

    let active = pool_repo
        .list_nodes(&NodeFilter::active_only(), None, 100)
        .await
        .expect("list active");
    assert_eq!(active.len(), 1, "TROJAN_B is missing, excluded");

    let all = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list all");
    assert_eq!(all.len(), 2);
    assert!(
        all.iter().any(|e| e.missing_from_source),
        "missing node visible with all() filter"
    );
}

/// `list_nodes` cursor pagination returns pages in ULID order.
#[tokio::test]
async fn list_nodes_paginates_by_cursor() {
    let db = TestDb::new().await;
    let pool_repo = SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );

    pool_repo
        .import_nodes(vec![trojan_node(TROJAN_A), trojan_node(TROJAN_B)])
        .await
        .expect("import");

    let page1 = pool_repo
        .list_nodes(&NodeFilter::all(), None, 1)
        .await
        .expect("page1");
    assert_eq!(page1.len(), 1);

    let cursor = page1.last().expect("page1 has one entry").node.id;
    let page2 = pool_repo
        .list_nodes(&NodeFilter::all(), Some(cursor), 1)
        .await
        .expect("page2");
    assert_eq!(page2.len(), 1);
    assert_ne!(page1[0].node.id, page2[0].node.id);
}

/// NODE-001: Manually imported nodes persist `source_label = "manual"` so
/// list views can distinguish them from source-bound nodes.
#[tokio::test]
async fn import_preserves_manual_source_label() {
    let db = TestDb::new().await;
    let pool_repo = SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );

    let mut node = trojan_node(TROJAN_A);
    node.source.source_label = "manual".to_owned();
    pool_repo.import_nodes(vec![node]).await.expect("import");

    let entries = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].node.source.source_label, "manual",
        "manually imported node should have source_label = 'manual'"
    );

    let entry = pool_repo
        .get_node(entries[0].node.id)
        .await
        .expect("get")
        .expect("node exists");
    assert_eq!(
        entry.node.source.source_label, "manual",
        "get_node should also return source_label = 'manual'"
    );
}
