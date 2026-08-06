#![allow(clippy::expect_used)]

//! Integration tests for `SqliteNodePoolRepository::reconcile`.
//!
//! Covers the core SRC-002/005/006/007/008/012/014 invariants: new node
//! insertion, dedup by (protocol, host, port), missing-node marking,
//! reactivation of previously-missing nodes, source_item recording, and
//! source binding creation. All within a single atomic transaction.

use deve_sub_domain::{
    ItemParseStatus, NodePoolRepository, ReconcileEntry, ReconcileInput, Source, SourceRepository,
    SourceSnapshot, SourceType,
};
use deve_sub_kernel::{NodeId, SourceId, SourceSnapshotId, Timestamp};
use deve_sub_storage_sqlite::{SqliteNodePoolRepository, SqliteSourceRepository};

struct TestDb {
    pool: sqlx::sqlite::SqlitePool,
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
        Self { pool, _dir: dir }
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

/// Parse a trojan URI into a Node with a fresh NodeId.
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
    "trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#NodeADup";

async fn count_nodes(pool: &sqlx::sqlite::SqlitePool) -> (i64, i64) {
    let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM nodes")
        .fetch_one(pool)
        .await
        .expect("count nodes");
    let (missing,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM nodes WHERE missing_from_source = 1")
            .fetch_one(pool)
            .await
            .expect("count missing");
    (total, missing)
}

async fn count_source_items(pool: &sqlx::sqlite::SqlitePool, snapshot_id: &str) -> i64 {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM source_items WHERE snapshot_id = ?")
            .bind(snapshot_id)
            .fetch_one(pool)
            .await
            .expect("count items");
    count
}

async fn count_bindings(pool: &sqlx::sqlite::SqlitePool, source_id: &str) -> i64 {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM node_source_bindings WHERE source_id = ?")
            .bind(source_id)
            .fetch_one(pool)
            .await
            .expect("count bindings");
    count
}

/// SRC-002/005: First refresh inserts all nodes as new.
#[tokio::test]
async fn first_refresh_inserts_new_nodes() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    let source = make_source("test-source");
    source_repo.create(&source).await.expect("create source");

    let entries = [entry(trojan_node(TROJAN_A)), entry(trojan_node(TROJAN_B))];
    let snapshot = make_snapshot(source.id, 1, entries.len() as u64);

    let result = pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snapshot,
            entries: &entries,
        })
        .await
        .expect("reconcile");

    assert_eq!(result.new_nodes, 2);
    assert_eq!(result.duplicate_nodes, 0);
    assert_eq!(result.reactivated_nodes, 0);
    assert_eq!(result.missing_nodes, 0);

    let (total, missing) = count_nodes(&db.pool).await;
    assert_eq!(total, 2);
    assert_eq!(missing, 0);

    assert_eq!(
        count_source_items(&db.pool, &snapshot.id.to_string()).await,
        2
    );
    assert_eq!(count_bindings(&db.pool, &source.id.to_string()).await, 2);
}

/// SRC-005: Duplicate nodes (same protocol+host+port) are recorded as
/// duplicates and do not create new pool entries.
#[tokio::test]
async fn duplicate_node_does_not_create_pool_entry() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    let source = make_source("test-source");
    source_repo.create(&source).await.expect("create source");

    let node_a = trojan_node(TROJAN_A);
    let node_dup = trojan_node(TROJAN_A_DUP);
    let entries = [entry(node_a), entry(node_dup)];
    let snapshot = make_snapshot(source.id, 1, 2);

    let result = pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snapshot,
            entries: &entries,
        })
        .await
        .expect("reconcile");

    assert_eq!(result.new_nodes, 1);
    assert_eq!(result.duplicate_nodes, 1);

    let (total, _) = count_nodes(&db.pool).await;
    assert_eq!(total, 1, "duplicate should not create a second pool node");
}

/// SRC-006: Nodes absent from a refresh are marked missing.
#[tokio::test]
async fn missing_nodes_marked_on_subsequent_refresh() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

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
    let result = pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snap_v2,
            entries: &entries_v2,
        })
        .await
        .expect("reconcile v2");

    assert_eq!(result.duplicate_nodes, 1, "TROJAN_A already in pool");
    assert_eq!(result.missing_nodes, 1, "TROJAN_B is now missing");

    let (total, missing) = count_nodes(&db.pool).await;
    assert_eq!(total, 2);
    assert_eq!(missing, 1);
}

/// SRC-006: A missing node that reappears in a later refresh is reactivated
/// (missing_from_source cleared) instead of inserted as new.
#[tokio::test]
async fn missing_node_reactivated_on_reappearance() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

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

    let entries_v3 = [entry(trojan_node(TROJAN_A)), entry(trojan_node(TROJAN_B))];
    let snap_v3 = make_snapshot(source.id, 3, 2);
    let result = pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snap_v3,
            entries: &entries_v3,
        })
        .await
        .expect("reconcile v3");

    assert_eq!(
        result.reactivated_nodes, 1,
        "TROJAN_B should be reactivated"
    );
    assert_eq!(
        result.duplicate_nodes, 1,
        "TROJAN_A still active, so duplicate"
    );

    let (total, missing) = count_nodes(&db.pool).await;
    assert_eq!(total, 2);
    assert_eq!(missing, 0, "no nodes missing after reactivation");
}

/// SRC-007: The new snapshot becomes the active one; the old is deactivated.
#[tokio::test]
async fn new_snapshot_replaces_active() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    let source = make_source("test-source");
    source_repo.create(&source).await.expect("create source");

    let snap_v1 = make_snapshot(source.id, 1, 1);
    pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snap_v1,
            entries: &[entry(trojan_node(TROJAN_A))],
        })
        .await
        .expect("reconcile v1");

    let snap_v2 = make_snapshot(source.id, 2, 1);
    pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snap_v2,
            entries: &[entry(trojan_node(TROJAN_A))],
        })
        .await
        .expect("reconcile v2");

    let (active_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM source_snapshots WHERE source_id = ? AND is_active = 1",
    )
    .bind(source.id.to_string())
    .fetch_one(&db.pool)
    .await
    .expect("count active");
    assert_eq!(active_count, 1, "exactly one active snapshot");

    let (active_id,): (String,) =
        sqlx::query_as("SELECT id FROM source_snapshots WHERE source_id = ? AND is_active = 1")
            .bind(source.id.to_string())
            .fetch_one(&db.pool)
            .await
            .expect("get active id");
    assert_eq!(active_id, snap_v2.id.to_string());
}

/// SRC-019: On failure, preserve the last successful subscription version.
/// A failed reconcile (e.g. duplicate binding constraint violation) must not
/// deactivate the old snapshot. We simulate by reconciling against a
/// non-existent source so FK fails.
#[tokio::test]
async fn failed_reconcile_preserves_old_snapshot() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    let source = make_source("test-source");
    source_repo.create(&source).await.expect("create source");

    let snap_v1 = make_snapshot(source.id, 1, 1);
    pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snap_v1,
            entries: &[entry(trojan_node(TROJAN_A))],
        })
        .await
        .expect("reconcile v1");

    let fake_source_id = SourceId::new();
    let snap_v2 = make_snapshot(fake_source_id, 2, 1);
    let result = pool_repo
        .reconcile(ReconcileInput {
            source_id: fake_source_id,
            snapshot: &snap_v2,
            entries: &[entry(trojan_node(TROJAN_A))],
        })
        .await;
    assert!(
        result.is_err(),
        "reconcile against non-existent source must fail"
    );

    let (active_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM source_snapshots WHERE source_id = ? AND is_active = 1",
    )
    .bind(source.id.to_string())
    .fetch_one(&db.pool)
    .await
    .expect("count active");
    assert_eq!(active_count, 1, "old snapshot still active after failure");

    let (total, missing) = count_nodes(&db.pool).await;
    assert_eq!(total, 1);
    assert_eq!(missing, 0, "pool unchanged after failure");
}

/// Multiple sources binding the same node: when one source drops it, the
/// node must NOT be marked missing (still bound by the other source).
#[tokio::test]
async fn node_bound_by_two_sources_not_missing_when_one_drops() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    let source_a = make_source("source-a");
    let source_b = make_source("source-b");
    source_repo.create(&source_a).await.expect("create a");
    source_repo.create(&source_b).await.expect("create b");

    let snap_a1 = make_snapshot(source_a.id, 1, 1);
    pool_repo
        .reconcile(ReconcileInput {
            source_id: source_a.id,
            snapshot: &snap_a1,
            entries: &[entry(trojan_node(TROJAN_A))],
        })
        .await
        .expect("reconcile a1");

    let snap_b1 = make_snapshot(source_b.id, 1, 1);
    pool_repo
        .reconcile(ReconcileInput {
            source_id: source_b.id,
            snapshot: &snap_b1,
            entries: &[entry(trojan_node(TROJAN_A))],
        })
        .await
        .expect("reconcile b1");

    let snap_b2 = make_snapshot(source_b.id, 2, 0);
    let result = pool_repo
        .reconcile(ReconcileInput {
            source_id: source_b.id,
            snapshot: &snap_b2,
            entries: &[],
        })
        .await
        .expect("reconcile b2 (empty)");

    assert_eq!(result.missing_nodes, 0, "node still bound by source_a");

    let (total, missing) = count_nodes(&db.pool).await;
    assert_eq!(total, 1);
    assert_eq!(missing, 0);
}
