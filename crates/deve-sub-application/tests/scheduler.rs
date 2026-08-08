#![allow(clippy::expect_used)]

//! Integration tests for `RefreshScheduler` (SRC-003).
//!
//! SRC-003 "自动刷新": the scheduler ticks at a configured interval, refreshes
//! sources whose `update_interval_secs` has elapsed, and does not double-fire
//! concurrent refreshes for the same source within a single tick.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use deve_sub_application::source::{
    self, CreateSourceParams, FetchError, FetchResult, GeoIpPort, RefreshScheduler,
    RegionDetection, SubscriptionFetcher,
};
use deve_sub_domain::{SourceRepository, SourceSnapshotRepository, SourceType};
use deve_sub_storage_sqlite::{
    SqliteNodePoolRepository, SqliteSourceRepository, SqliteSourceSnapshotRepository,
};

struct CountingFetcher {
    calls: Arc<AtomicU32>,
    body: Vec<u8>,
}

impl CountingFetcher {
    fn new(body: Vec<u8>) -> (Self, Arc<AtomicU32>) {
        let calls = Arc::new(AtomicU32::new(0));
        (
            Self {
                calls: calls.clone(),
                body,
            },
            calls,
        )
    }
}

#[async_trait]
impl SubscriptionFetcher for CountingFetcher {
    async fn fetch(&self, _url: &str, _etag: Option<&str>) -> Result<FetchResult, FetchError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(FetchResult::Ok {
            body: self.body.clone(),
            etag: None,
            content_type: Some("text/plain".to_owned()),
        })
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

const TROJAN_URI: &str = "trojan://PASS@example.com:443?sni=example.com&type=tcp#Node";

async fn create_auto_source(repo: &SqliteSourceRepository, name: &str, interval_secs: u64) {
    source::create_source(
        repo,
        CreateSourceParams {
            name: name.to_owned(),
            source_type: SourceType::UriList,
            url: "https://example.com/sub".to_owned(),
            auto_update: true,
            update_interval_secs: interval_secs,
            keep_on_fail: true,
            filter_rules: None,
        },
    )
    .await
    .expect("create source");
}

/// SRC-003: The scheduler refreshes a due source on the first tick.
#[tokio::test]
async fn scheduler_refreshes_due_source_on_tick() {
    let db = TestDb::new().await;
    let source_repo = Arc::new(SqliteSourceRepository::new(db.pool.clone()));
    let snapshot_repo = Arc::new(SqliteSourceSnapshotRepository::new(db.pool.clone()));
    let pool_repo = Arc::new(SqliteNodePoolRepository::new(db.pool.clone()));
    let (fetcher, calls) = CountingFetcher::new(TROJAN_URI.as_bytes().to_vec());

    create_auto_source(&source_repo, "auto-source", 3600).await;

    let scheduler = RefreshScheduler::new(
        source_repo.clone(),
        snapshot_repo.clone(),
        pool_repo.clone(),
        Arc::new(fetcher),
        Arc::new(StubGeoIp),
    )
    .tick_interval(Duration::from_millis(50));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    let handle = tokio::spawn(scheduler.run(shutdown));

    tokio::time::sleep(Duration::from_millis(150)).await;
    let _ = shutdown_tx.send(());
    let _ = handle.await;

    assert!(
        calls.load(Ordering::SeqCst) >= 1,
        "scheduler should have refreshed the due source at least once"
    );

    let sources = source_repo.list(None, 100).await.expect("list sources");
    assert_eq!(sources.len(), 1);
    let snapshots = snapshot_repo
        .list_for_source(sources[0].id, 100)
        .await
        .expect("list snapshots");
    assert!(!snapshots.is_empty(), "at least one snapshot created");
}

/// SRC-003: A source that is not yet due is not refreshed.
#[tokio::test]
async fn scheduler_skips_not_due_source() {
    let db = TestDb::new().await;
    let source_repo = Arc::new(SqliteSourceRepository::new(db.pool.clone()));
    let snapshot_repo = Arc::new(SqliteSourceSnapshotRepository::new(db.pool.clone()));
    let pool_repo = Arc::new(SqliteNodePoolRepository::new(db.pool.clone()));
    let (fetcher, calls) = CountingFetcher::new(TROJAN_URI.as_bytes().to_vec());

    // interval is 1 hour; the source was just created so it has no snapshot
    // yet — but `collect_due_sources` treats "no snapshot" as due. So we
    // first do a manual refresh, then verify the scheduler does not refresh
    // again immediately.
    create_auto_source(&source_repo, "auto-source", 3600).await;
    let sid = source_repo.list(None, 1).await.expect("list")[0].id;
    source::refresh_source(
        source_repo.as_ref(),
        snapshot_repo.as_ref(),
        pool_repo.as_ref(),
        &*Arc::new(CountingFetcher::new(TROJAN_URI.as_bytes().to_vec()).0),
        &StubGeoIp,
        sid,
    )
    .await
    .expect("manual refresh");

    let scheduler = RefreshScheduler::new(
        source_repo.clone(),
        snapshot_repo.clone(),
        pool_repo.clone(),
        Arc::new(fetcher),
        Arc::new(StubGeoIp),
    )
    .tick_interval(Duration::from_millis(50));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    let handle = tokio::spawn(scheduler.run(shutdown));

    tokio::time::sleep(Duration::from_millis(150)).await;
    let _ = shutdown_tx.send(());
    let _ = handle.await;

    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "scheduler should not refresh a source that is not due"
    );
}

/// SRC-003: A disabled source (even if due) is not refreshed.
#[tokio::test]
async fn scheduler_skips_disabled_source() {
    let db = TestDb::new().await;
    let source_repo = Arc::new(SqliteSourceRepository::new(db.pool.clone()));
    let snapshot_repo = Arc::new(SqliteSourceSnapshotRepository::new(db.pool.clone()));
    let pool_repo = Arc::new(SqliteNodePoolRepository::new(db.pool.clone()));
    let (fetcher, calls) = CountingFetcher::new(TROJAN_URI.as_bytes().to_vec());

    create_auto_source(&source_repo, "auto-source", 1).await;
    // Disable the source.
    let mut s = source_repo.list(None, 1).await.expect("list")[0].clone();
    s.enabled = false;
    source_repo.update(&s).await.expect("disable");

    let scheduler = RefreshScheduler::new(
        source_repo.clone(),
        snapshot_repo.clone(),
        pool_repo.clone(),
        Arc::new(fetcher),
        Arc::new(StubGeoIp),
    )
    .tick_interval(Duration::from_millis(50));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    let handle = tokio::spawn(scheduler.run(shutdown));

    tokio::time::sleep(Duration::from_millis(150)).await;
    let _ = shutdown_tx.send(());
    let _ = handle.await;

    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "disabled source should not be refreshed"
    );
}

/// SRC-003: Shutdown signal stops the scheduler cleanly.
#[tokio::test]
async fn scheduler_stops_on_shutdown() {
    let db = TestDb::new().await;
    let source_repo = Arc::new(SqliteSourceRepository::new(db.pool.clone()));
    let snapshot_repo = Arc::new(SqliteSourceSnapshotRepository::new(db.pool.clone()));
    let pool_repo = Arc::new(SqliteNodePoolRepository::new(db.pool.clone()));
    let (fetcher, _calls) = CountingFetcher::new(TROJAN_URI.as_bytes().to_vec());

    create_auto_source(&source_repo, "auto-source", 1).await;

    let scheduler = RefreshScheduler::new(
        source_repo,
        snapshot_repo,
        pool_repo,
        Arc::new(fetcher),
        Arc::new(StubGeoIp),
    )
    .tick_interval(Duration::from_secs(60));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    let handle = tokio::spawn(scheduler.run(shutdown));

    let _ = shutdown_tx.send(());
    let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
    assert!(
        result.is_ok(),
        "scheduler should stop within 5s of shutdown"
    );
}
