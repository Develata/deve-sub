#![allow(clippy::expect_used)]

//! Regression tests for at-rest encryption of node credential JSON columns.
//! See ADR-0007 and DS-AUD-027.

use std::sync::Arc;

use deve_sub_domain::{
    ItemParseStatus, NodeFilter, NodePoolRepository, ReconcileEntry, ReconcileInput, Source,
    SourceRepository, SourceSnapshot, SourceType,
};
use deve_sub_kernel::{NodeId, SourceId, SourceSnapshotId, Timestamp};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{SqliteNodePoolRepository, SqliteSourceRepository};

const TROJAN: &str = "trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#NodeA";

type EncryptedColumns = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

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
            master_key: Arc::new(MasterKey::from_bytes(&[0x42; 32])),
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

/// DS-AUD-027: When a master key is configured, sensitive JSON columns
/// (authentication, protocol_config, tls, transport, obfuscation, extras)
/// must be persisted as v2 secret envelopes, not plaintext.
#[tokio::test]
async fn node_credentials_encrypted_at_rest() {
    let db = TestDb::new().await;
    let source_repo =
        SqliteSourceRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));

    let source = make_source("test-source");
    source_repo.create(&source).await.expect("create source");

    let node = trojan_node(TROJAN);
    let entries = [entry(node)];
    let snapshot = make_snapshot(source.id, 1, 1);

    pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snapshot,
            entries: &entries,
        })
        .await
        .expect("reconcile");

    let row: EncryptedColumns = sqlx::query_as(
        "SELECT authentication_json_encrypted, protocol_config_json_encrypted, \
              tls_json_encrypted, transport_json_encrypted, obfuscation_json_encrypted, \
              extras_json_encrypted FROM nodes LIMIT 1",
    )
    .fetch_one(&db.pool)
    .await
    .expect("fetch encrypted columns");

    let (auth_enc, cfg_enc, tls_enc, tp_enc, obf_enc, ext_enc) = row;
    assert!(
        auth_enc.is_some(),
        "authentication_json_encrypted must be populated"
    );
    assert!(
        cfg_enc.is_some(),
        "protocol_config_json_encrypted must be populated"
    );
    assert!(ext_enc.is_some(), "extras_json_encrypted must be populated");

    for (label, enc) in [
        ("auth", auth_enc.as_deref()),
        ("cfg", cfg_enc.as_deref()),
        ("tls", tls_enc.as_deref()),
        ("tp", tp_enc.as_deref()),
        ("obf", obf_enc.as_deref()),
        ("ext", ext_enc.as_deref()),
    ] {
        if let Some(e) = enc {
            assert!(
                e.starts_with("v2:"),
                "{label} envelope must have v2: prefix"
            );
            assert!(
                !e.contains("TEST_PASSWORD"),
                "{label} envelope must not leak plaintext"
            );
        }
    }
}

/// DS-AUD-027: Reading nodes back with the same key decrypts the sensitive
/// fields and reconstructs the original credential JSON.
#[tokio::test]
async fn read_decrypts_node_credentials() {
    let db = TestDb::new().await;
    let source_repo =
        SqliteSourceRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));

    let source = make_source("test-source");
    source_repo.create(&source).await.expect("create source");

    let node = trojan_node(TROJAN);
    let entries = [entry(node)];
    let snapshot = make_snapshot(source.id, 1, 1);

    pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snapshot,
            entries: &entries,
        })
        .await
        .expect("reconcile");

    let nodes = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list nodes");
    assert_eq!(nodes.len(), 1);
    let entry = &nodes[0];
    let auth_json = serde_json::to_string(&entry.node.authentication).expect("serialize auth");
    assert!(
        auth_json.contains("TEST_PASSWORD"),
        "decrypted authentication must contain original password"
    );
}

/// DS-AUD-027: Without a master key, reading encrypted nodes fails closed
/// rather than silently returning empty or garbage credentials.
#[tokio::test]
async fn no_key_read_fails_closed() {
    let db = TestDb::new().await;
    let source_repo =
        SqliteSourceRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));

    let source = make_source("test-source");
    source_repo.create(&source).await.expect("create source");

    let node = trojan_node(TROJAN);
    let entries = [entry(node)];
    let snapshot = make_snapshot(source.id, 1, 1);

    pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snapshot,
            entries: &entries,
        })
        .await
        .expect("reconcile");

    let no_key_repo = SqliteNodePoolRepository::new(db.pool.clone());
    let result = no_key_repo.list_nodes(&NodeFilter::all(), None, 100).await;
    assert!(
        result.is_err(),
        "reading encrypted nodes without a key must error (fail-closed)"
    );
}

/// DS-AUD-027 (raw_uri surface): When a master key is configured, raw share
/// URIs recorded in `source_items` and `node_source_bindings` are stored as
/// v2 secret envelopes. Envelopes must have the v2: prefix and must not leak
/// the original password.
#[tokio::test]
async fn raw_uri_encrypted_in_items_and_bindings() {
    let db = TestDb::new().await;
    let source_repo =
        SqliteSourceRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));

    let source = make_source("test-source");
    source_repo.create(&source).await.expect("create source");

    let node = trojan_node(TROJAN);
    let entries = [entry(node)];
    let snapshot = make_snapshot(source.id, 1, 1);

    pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &snapshot,
            entries: &entries,
        })
        .await
        .expect("reconcile");

    let item_row: (Option<String>,) =
        sqlx::query_as("SELECT raw_uri_encrypted FROM source_items LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .expect("fetch item");
    let item_enc = item_row
        .0
        .expect("source_items.raw_uri_encrypted populated");
    assert!(item_enc.starts_with("v2:"));
    assert!(
        !item_enc.contains("TEST_PASSWORD"),
        "source_items envelope must not leak plaintext"
    );

    let binding_row: (Option<String>,) =
        sqlx::query_as("SELECT raw_uri_encrypted FROM node_source_bindings LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .expect("fetch binding");
    let binding_enc = binding_row
        .0
        .expect("node_source_bindings.raw_uri_encrypted populated");
    assert!(binding_enc.starts_with("v2:"));
    assert!(
        !binding_enc.contains("TEST_PASSWORD"),
        "node_source_bindings envelope must not leak plaintext"
    );
}
