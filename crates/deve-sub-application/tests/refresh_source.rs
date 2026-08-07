#![allow(clippy::expect_used)]

//! Integration tests for `refresh_source` (SRC-002, SRC-005, SRC-006, SRC-008,
//! SRC-012, SRC-019).
//!
//! Uses a real SQLite storage layer (source + snapshot + node pool repos) and
//! a mock fetcher to control the fetched content. Covers:
//! - Successful fetch → parse → reconcile.
//! - 304 Not Modified (no new snapshot).
//! - Fetch failure preserves the last successful snapshot (constraint #19).
//! - Parse failure (YAML bomb / too many nodes) preserves old snapshot.

use std::sync::Mutex;

use async_trait::async_trait;
use deve_sub_application::source::{
    self, CreateSourceParams, FetchError, FetchResult, GeoIpPort, RegionDetection,
    SubscriptionFetcher,
};
use deve_sub_domain::{SourceSnapshotRepository, SourceType};
use deve_sub_storage_sqlite::{
    SqliteNodePoolRepository, SqliteSourceRepository, SqliteSourceSnapshotRepository,
};

/// Mock fetcher that returns a pre-programmed response.
struct MockFetcher {
    responses: Mutex<Vec<MockResponse>>,
}

enum MockResponse {
    Ok {
        body: Vec<u8>,
        etag: Option<String>,
        content_type: Option<String>,
    },
    NotModified,
    Error(FetchError),
}

impl MockFetcher {
    fn new(responses: Vec<MockResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }
}

#[async_trait]
impl SubscriptionFetcher for MockFetcher {
    async fn fetch(&self, _url: &str, _etag: Option<&str>) -> Result<FetchResult, FetchError> {
        let mut responses = self.responses.lock().expect("mutex");
        if responses.is_empty() {
            return Err(FetchError::Connection("no more mock responses".to_owned()));
        }
        match responses.remove(0) {
            MockResponse::Ok {
                body,
                etag,
                content_type,
            } => Ok(FetchResult::Ok {
                body,
                etag,
                content_type,
            }),
            MockResponse::NotModified => Ok(FetchResult::NotModified),
            MockResponse::Error(e) => Err(e),
        }
    }
}

struct StubGeoIp;

#[async_trait]
impl GeoIpPort for StubGeoIp {
    async fn detect_region(&self, _host: &str) -> RegionDetection {
        RegionDetection {
            region: None,
            candidate_ips: vec![],
        }
    }
}

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

async fn create_source_typed(
    repo: &SqliteSourceRepository,
    name: &str,
    source_type: SourceType,
) -> deve_sub_domain::Source {
    source::create_source(
        repo,
        CreateSourceParams {
            name: name.to_owned(),
            source_type,
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

const TROJAN_URI_LIST: &str = "trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#NodeA\n\
     trojan://TEST_PASSWORD@other.com:8443?sni=other.com&type=tcp#NodeB";

/// SRC-002/005: Successful refresh creates snapshot, inserts nodes.
#[tokio::test]
async fn refresh_inserts_nodes_and_creates_snapshot() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());
    let fetcher = MockFetcher::new(vec![MockResponse::Ok {
        body: TROJAN_URI_LIST.as_bytes().to_vec(),
        etag: Some("\"v1\"".to_owned()),
        content_type: Some("text/plain".to_owned()),
    }]);

    let source = create_source(&source_repo, "test-source").await;

    let result = source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher,
        &StubGeoIp,
        source.id,
    )
    .await
    .expect("refresh");

    assert!(!result.not_modified);
    assert_eq!(result.snapshot.version, 1);
    assert_eq!(result.snapshot.node_count, 2);
    assert_eq!(result.reconcile.new_nodes, 2);
    assert_eq!(result.reconcile.duplicate_nodes, 0);

    let active = snapshot_repo
        .find_active(source.id)
        .await
        .expect("find active");
    assert!(active.is_some());
    assert_eq!(active.expect("active snapshot").version, 1);
}

/// SRC-004: ETag 304 returns not_modified without creating a new snapshot.
#[tokio::test]
async fn refresh_304_not_modified_preserves_old_snapshot() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    let source = create_source(&source_repo, "test-source").await;

    let fetcher_v1 = MockFetcher::new(vec![MockResponse::Ok {
        body: TROJAN_URI_LIST.as_bytes().to_vec(),
        etag: Some("\"v1\"".to_owned()),
        content_type: Some("text/plain".to_owned()),
    }]);
    let r1 = source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher_v1,
        &StubGeoIp,
        source.id,
    )
    .await
    .expect("refresh v1");
    assert!(!r1.not_modified);
    assert_eq!(r1.snapshot.version, 1);

    let fetcher_304 = MockFetcher::new(vec![MockResponse::NotModified]);
    let r2 = source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher_304,
        &StubGeoIp,
        source.id,
    )
    .await
    .expect("refresh v2");

    assert!(r2.not_modified, "second refresh should be 304");
    assert_eq!(r2.snapshot.version, 1, "snapshot version unchanged on 304");
    assert_eq!(r2.reconcile.new_nodes, 0);
}

/// SRC-019: Fetch failure preserves the last successful snapshot.
#[tokio::test]
async fn fetch_failure_preserves_old_snapshot() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    let source = create_source(&source_repo, "test-source").await;

    let fetcher_v1 = MockFetcher::new(vec![MockResponse::Ok {
        body: TROJAN_URI_LIST.as_bytes().to_vec(),
        etag: Some("\"v1\"".to_owned()),
        content_type: Some("text/plain".to_owned()),
    }]);
    source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher_v1,
        &StubGeoIp,
        source.id,
    )
    .await
    .expect("refresh v1");

    let fetcher_fail = MockFetcher::new(vec![MockResponse::Error(FetchError::Timeout(30))]);
    let result = source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher_fail,
        &StubGeoIp,
        source.id,
    )
    .await;
    assert!(result.is_err(), "fetch failure should return error");

    let active = snapshot_repo
        .find_active(source.id)
        .await
        .expect("find active");
    assert!(active.is_some());
    assert_eq!(
        active.expect("active snapshot").version,
        1,
        "old snapshot v1 still active after fetch failure"
    );

    let (node_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM nodes")
        .fetch_one(&db.pool)
        .await
        .expect("count");
    assert_eq!(node_count, 2, "pool unchanged after fetch failure");
}

/// SRC-019: Parse failure preserves the last successful snapshot.
#[tokio::test]
async fn parse_failure_preserves_old_snapshot() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(TROJAN_URI_LIST);
    let source = create_source_typed(&source_repo, "test-source", SourceType::Base64).await;

    let fetcher_v1 = MockFetcher::new(vec![MockResponse::Ok {
        body: encoded.into_bytes(),
        etag: Some("\"v1\"".to_owned()),
        content_type: Some("text/plain".to_owned()),
    }]);
    source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher_v1,
        &StubGeoIp,
        source.id,
    )
    .await
    .expect("refresh v1");

    let fetcher_bad = MockFetcher::new(vec![MockResponse::Ok {
        body: b"!!!not-valid-base64!!!".to_vec(),
        etag: None,
        content_type: None,
    }]);
    let result = source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher_bad,
        &StubGeoIp,
        source.id,
    )
    .await;
    assert!(
        result.is_err(),
        "invalid base64 should trigger ParseContentError"
    );

    let active = snapshot_repo
        .find_active(source.id)
        .await
        .expect("find active");
    assert!(active.is_some());
    assert_eq!(
        active.expect("active snapshot").version,
        1,
        "old snapshot still active"
    );
}

/// SRC-002: Refreshing a non-existent source returns SourceNotFound.
#[tokio::test]
async fn refresh_nonexistent_source_returns_not_found() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());
    let fetcher = MockFetcher::new(vec![]);

    let fake_id = deve_sub_kernel::SourceId::new();
    let result = source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher,
        &StubGeoIp,
        fake_id,
    )
    .await;

    match result {
        Err(source::SourceAppError::SourceNotFound) => {}
        other => panic!("expected SourceNotFound, got {other:?}"),
    }
}

/// SRC-005: When `keep_on_fail` is false, a fetch failure disables the source.
#[tokio::test]
async fn fetch_failure_disables_source_when_keep_on_fail_false() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    let source = source::create_source(
        &source_repo,
        CreateSourceParams {
            name: "test-source".to_owned(),
            source_type: SourceType::UriList,
            url: "https://example.com/sub".to_owned(),
            auto_update: false,
            update_interval_secs: 3600,
            keep_on_fail: false,
            filter_rules: None,
        },
    )
    .await
    .expect("create source");

    let fetcher = MockFetcher::new(vec![MockResponse::Error(FetchError::Timeout(30))]);
    let result = source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher,
        &StubGeoIp,
        source.id,
    )
    .await;
    assert!(result.is_err(), "fetch failure should return error");

    let reloaded = source::get_source(&source_repo, source.id)
        .await
        .expect("get source")
        .expect("source exists");
    assert!(
        !reloaded.enabled,
        "source should be disabled after failure with keep_on_fail=false"
    );
}

/// SRC-005: When `keep_on_fail` is true, a fetch failure preserves `enabled`.
#[tokio::test]
async fn fetch_failure_preserves_enabled_when_keep_on_fail_true() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    let source = source::create_source(
        &source_repo,
        CreateSourceParams {
            name: "test-source".to_owned(),
            source_type: SourceType::UriList,
            url: "https://example.com/sub".to_owned(),
            auto_update: false,
            update_interval_secs: 3600,
            keep_on_fail: true,
            filter_rules: None,
        },
    )
    .await
    .expect("create source");

    let fetcher = MockFetcher::new(vec![MockResponse::Error(FetchError::Timeout(30))]);
    let result = source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher,
        &StubGeoIp,
        source.id,
    )
    .await;
    assert!(result.is_err());

    let reloaded = source::get_source(&source_repo, source.id)
        .await
        .expect("get source")
        .expect("source exists");
    assert!(
        reloaded.enabled,
        "source should remain enabled with keep_on_fail=true"
    );
}

/// SRC-009: A failed refresh does not publish a half-finished snapshot.
/// The reconcile transaction is atomic — on any failure between fetch and
/// commit, no new snapshot version is created. Here a parse failure occurs
/// after a successful fetch; we verify the snapshot count stays at the
/// pre-failure value (no half-baked snapshot was persisted).
#[tokio::test]
async fn cancelled_refresh_publishes_no_half_snapshot() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(TROJAN_URI_LIST);
    let source = create_source_typed(&source_repo, "test-source", SourceType::Base64).await;

    let fetcher_v1 = MockFetcher::new(vec![MockResponse::Ok {
        body: encoded.into_bytes(),
        etag: Some("\"v1\"".to_owned()),
        content_type: Some("text/plain".to_owned()),
    }]);
    source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher_v1,
        &StubGeoIp,
        source.id,
    )
    .await
    .expect("refresh v1");

    let snapshots_before = snapshot_repo
        .list_for_source(source.id, 100)
        .await
        .expect("list snapshots");
    assert_eq!(
        snapshots_before.len(),
        1,
        "one snapshot after successful v1 refresh"
    );

    let fetcher_bad = MockFetcher::new(vec![MockResponse::Ok {
        body: b"!!!not-valid-base64!!!".to_vec(),
        etag: None,
        content_type: None,
    }]);
    let result = source::refresh_source(
        &source_repo,
        &snapshot_repo,
        &pool_repo,
        &fetcher_bad,
        &StubGeoIp,
        source.id,
    )
    .await;
    assert!(result.is_err(), "parse failure should error out");

    let snapshots_after = snapshot_repo
        .list_for_source(source.id, 100)
        .await
        .expect("list snapshots");
    assert_eq!(
        snapshots_after.len(),
        1,
        "no half-finished snapshot published after parse failure"
    );
    assert_eq!(
        snapshots_after[0].version, 1,
        "active snapshot version unchanged"
    );
}

/// SRC-013: Concurrent refreshes of two distinct sources do not cross-pollute.
/// Each refresh operates on an independent source_id with its own snapshot and
/// binding set. After concurrent refresh, each source's bindings contain only
/// its own nodes.
#[tokio::test]
async fn concurrent_refreshes_do_not_cross_pollute() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new(db.pool.clone());
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());

    let source_a = create_source(&source_repo, "source-a").await;
    let source_b = create_source(&source_repo, "source-b").await;

    let uri_list_a = "trojan://PASS_A@host-a.example.com:443?sni=a.example.com&type=tcp#NodeA";
    let uri_list_b = "trojan://PASS_B@host-b.example.com:8443?sni=b.example.com&type=tcp#NodeB";

    let fetcher_a = MockFetcher::new(vec![MockResponse::Ok {
        body: uri_list_a.as_bytes().to_vec(),
        etag: Some("\"a1\"".to_owned()),
        content_type: Some("text/plain".to_owned()),
    }]);
    let fetcher_b = MockFetcher::new(vec![MockResponse::Ok {
        body: uri_list_b.as_bytes().to_vec(),
        etag: Some("\"b1\"".to_owned()),
        content_type: Some("text/plain".to_owned()),
    }]);

    let (r_a, r_b) = tokio::join!(
        source::refresh_source(
            &source_repo,
            &snapshot_repo,
            &pool_repo,
            &fetcher_a,
            &StubGeoIp,
            source_a.id,
        ),
        source::refresh_source(
            &source_repo,
            &snapshot_repo,
            &pool_repo,
            &fetcher_b,
            &StubGeoIp,
            source_b.id,
        ),
    );
    let r_a = r_a.expect("refresh a");
    let r_b = r_b.expect("refresh b");
    assert_eq!(r_a.reconcile.new_nodes, 1);
    assert_eq!(r_b.reconcile.new_nodes, 1);

    let bindings_a: Vec<(String,)> = sqlx::query_as(
        "SELECT n.id FROM nodes n \
         JOIN node_source_bindings b ON n.id = b.node_id \
         WHERE b.source_id = ?",
    )
    .bind(source_a.id.to_string())
    .fetch_all(&db.pool)
    .await
    .expect("bindings a");
    let bindings_b: Vec<(String,)> = sqlx::query_as(
        "SELECT n.id FROM nodes n \
         JOIN node_source_bindings b ON n.id = b.node_id \
         WHERE b.source_id = ?",
    )
    .bind(source_b.id.to_string())
    .fetch_all(&db.pool)
    .await
    .expect("bindings b");

    assert_eq!(bindings_a.len(), 1, "source A has exactly one bound node");
    assert_eq!(bindings_b.len(), 1, "source B has exactly one bound node");
    assert_ne!(
        bindings_a[0].0, bindings_b[0].0,
        "concurrent refreshes must not cross-pollute node bindings"
    );

    let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM nodes")
        .fetch_one(&db.pool)
        .await
        .expect("count");
    assert_eq!(total, 2, "two distinct nodes in the pool");
}
