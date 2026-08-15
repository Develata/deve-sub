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
/// must be persisted as secret envelopes, not plaintext.
#[tokio::test]
async fn node_credentials_encrypted_at_rest() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let key = Arc::new(MasterKey::from_bytes(&[0x42; 32]));
    let pool_repo = SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&key));

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

    let row: (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT authentication_json_encrypted, protocol_config_json_encrypted, \
             tls_json_encrypted, transport_json_encrypted, obfuscation_json_encrypted, \
             extras_json_encrypted FROM nodes LIMIT 1",
    )
    .fetch_one(&db.pool)
    .await
    .expect("fetch encrypted columns");

    let plaintext_row: (String, String) =
        sqlx::query_as("SELECT authentication_json, extras_json FROM nodes LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .expect("fetch plaintext columns");

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
                e.starts_with("v1:"),
                "{label} envelope must have v1: prefix"
            );
            assert!(
                !e.contains("TEST_PASSWORD"),
                "{label} envelope must not leak plaintext"
            );
        }
    }

    let (auth_plain, ext_plain) = plaintext_row;
    assert!(
        auth_plain.contains("TEST_PASSWORD"),
        "plaintext auth column retained for backward compat"
    );
    assert!(
        ext_plain.contains("tcp") || !ext_plain.is_empty(),
        "plaintext extras column retained for backward compat"
    );
}

/// DS-AUD-027: Reading nodes back with the same key decrypts the sensitive
/// fields and reconstructs the original credential JSON.
#[tokio::test]
async fn read_decrypts_node_credentials() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let key = Arc::new(MasterKey::from_bytes(&[0x42; 32]));
    let pool_repo = SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&key));

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

/// DS-AUD-027: Without a master key, the repository falls back to plaintext
/// columns (legacy compatibility path).
#[tokio::test]
async fn no_key_falls_back_to_plaintext() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

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

    let (enc_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM nodes WHERE authentication_json_encrypted IS NULL")
            .fetch_one(&db.pool)
            .await
            .expect("count");
    assert_eq!(
        enc_count, 1,
        "no key → encrypted columns stay NULL, plaintext used"
    );

    let nodes = pool_repo
        .list_nodes(&NodeFilter::all(), None, 100)
        .await
        .expect("list nodes");
    assert_eq!(nodes.len(), 1);
    let auth_json = serde_json::to_string(&nodes[0].node.authentication).expect("serialize auth");
    assert!(
        auth_json.contains("TEST_PASSWORD"),
        "plaintext fallback must still expose original credentials"
    );
}
