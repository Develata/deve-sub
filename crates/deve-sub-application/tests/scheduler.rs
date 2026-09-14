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
    self, CreateSourceParams, FetchError, FetchResult, GeoIpPort, RefreshDeps, RefreshScheduler,
    RegionDetection, SubscriptionFetcher, execute_refresh_job, start_refresh_job,
};
use deve_sub_domain::{
    SourceRefreshJobRepository, SourceRefreshJobStatus, SourceRepository, SourceSnapshotRepository,
    SourceType,
};
use deve_sub_kernel::SourceId;
use deve_sub_storage_sqlite::{
    SqliteNodePoolRepository, SqliteSourceRefreshJobRepository, SqliteSourceRepository,
    SqliteSourceSnapshotRepository,
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

const TROJAN_URI: &str = "trojan://PASS@example.com:443?sni=example.com&type=tcp#Node";

async fn create_auto_source(
    repo: &SqliteSourceRepository,
    name: &str,
    interval_secs: u64,
) -> SourceId {
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
    .expect("create source")
    .id
}

// WHY: elapsed wall time cannot prove a tick ran under SQLite contention.
// Observe durable completion before shutdown; failed jobs remain test failures.
async fn wait_for_completed_refresh(db: &TestDb, source_id: SourceId) {
    let jobs = SqliteSourceRefreshJobRepository::new(db.pool.clone());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let current = jobs.list_for_source(source_id, 1).await.expect("jobs");
            if let Some(job) = current.first()
                && job.status.is_terminal()
            {
                assert_eq!(job.status, SourceRefreshJobStatus::Completed, "{job:?}");
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("scheduler must complete the due source within 5s");
}

async fn stop_scheduler(
    stop: tokio::sync::oneshot::Sender<()>,
    mut handle: tokio::task::JoinHandle<()>,
) {
    stop.send(()).expect("scheduler is still running");
    let result = tokio::time::timeout(Duration::from_secs(5), &mut handle).await;
    if result.is_err() {
        handle.abort();
        let _ = handle.await;
    }
    result
        .expect("scheduler shutdown within 5s")
        .expect("scheduler task");
}

/// SRC-003: The scheduler refreshes a due source on the first tick.
#[tokio::test]
async fn scheduler_refreshes_due_source_on_tick() {
    let db = TestDb::new().await;
    let source_repo = Arc::new(SqliteSourceRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    ));
    let snapshot_repo = Arc::new(SqliteSourceSnapshotRepository::new(db.pool.clone()));
    let pool_repo = Arc::new(SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    ));
    let (fetcher, calls) = CountingFetcher::new(TROJAN_URI.as_bytes().to_vec());

    let source_id = create_auto_source(&source_repo, "auto-source", 3600).await;

    let scheduler = RefreshScheduler::new(
        source_repo.clone(),
        snapshot_repo.clone(),
        pool_repo.clone(),
        Arc::new(SqliteSourceRefreshJobRepository::new(db.pool.clone())),
        Arc::new(fetcher),
        Arc::new(StubGeoIp),
    )
    .tick_interval(Duration::from_millis(50));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    let handle = tokio::spawn(scheduler.run(shutdown));

    wait_for_completed_refresh(&db, source_id).await;
    stop_scheduler(shutdown_tx, handle).await;

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "scheduler must refresh the due source exactly once"
    );

    let sources = source_repo.list(None, 100).await.expect("list sources");
    assert_eq!(sources.len(), 1);
    let snapshots = snapshot_repo
        .list_for_source(sources[0].id, 100)
        .await
        .expect("list snapshots");
    assert_eq!(snapshots.len(), 1, "exactly one snapshot created");
}

/// SRC-003: A source that is not yet due is not refreshed.
#[tokio::test]
async fn scheduler_skips_not_due_source() {
    let db = TestDb::new().await;
    let source_repo = Arc::new(SqliteSourceRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    ));
    let snapshot_repo = Arc::new(SqliteSourceSnapshotRepository::new(db.pool.clone()));
    let pool_repo = Arc::new(SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    ));
    let (fetcher, calls) = CountingFetcher::new(TROJAN_URI.as_bytes().to_vec());

    // interval is 1 hour; the source was just created so it has no snapshot
    // yet — but `collect_due_sources` treats "no snapshot" as due. So we
    // first do a manual refresh, then verify the scheduler does not refresh
    // again immediately.
    create_auto_source(&source_repo, "auto-source", 3600).await;
    let sid = source_repo.list(None, 1).await.expect("list")[0].id;
    {
        let job_repo = SqliteSourceRefreshJobRepository::new(db.pool.clone());
        let manual_fetcher = CountingFetcher::new(TROJAN_URI.as_bytes().to_vec()).0;
        let deps = RefreshDeps {
            source_repo: source_repo.as_ref(),
            snapshot_repo: snapshot_repo.as_ref(),
            pool_repo: pool_repo.as_ref(),
            job_repo: &job_repo,
            fetcher: &manual_fetcher,
            geoip: &StubGeoIp,
        };
        let job_id = start_refresh_job(&deps, sid).await.expect("start job");
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        execute_refresh_job(&deps, job_id, sid, &cancelled)
            .await
            .expect("manual refresh");
    }

    // A due control proves the scheduler actually scanned this negative case.
    let control = create_auto_source(&source_repo, "due-control", 3600).await;
    let scheduler = RefreshScheduler::new(
        source_repo.clone(),
        snapshot_repo.clone(),
        pool_repo.clone(),
        Arc::new(SqliteSourceRefreshJobRepository::new(db.pool.clone())),
        Arc::new(fetcher),
        Arc::new(StubGeoIp),
    )
    .tick_interval(Duration::from_millis(50));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    let handle = tokio::spawn(scheduler.run(shutdown));

    wait_for_completed_refresh(&db, control).await;
    stop_scheduler(shutdown_tx, handle).await;

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "only the due control may be refreshed"
    );
    assert_eq!(
        snapshot_repo
            .list_for_source(sid, 100)
            .await
            .expect("snapshots")
            .len(),
        1
    );
    assert_eq!(
        SqliteSourceRefreshJobRepository::new(db.pool.clone())
            .list_for_source(sid, 100)
            .await
            .expect("jobs")
            .len(),
        1,
        "not-due source retains only its earlier manual job"
    );
}

/// SRC-003: A disabled source (even if due) is not refreshed.
#[tokio::test]
async fn scheduler_skips_disabled_source() {
    let db = TestDb::new().await;
    let source_repo = Arc::new(SqliteSourceRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    ));
    let snapshot_repo = Arc::new(SqliteSourceSnapshotRepository::new(db.pool.clone()));
    let pool_repo = Arc::new(SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    ));
    let (fetcher, calls) = CountingFetcher::new(TROJAN_URI.as_bytes().to_vec());

    create_auto_source(&source_repo, "auto-source", 1).await;
    // Disable the source.
    let mut s = source_repo.list(None, 1).await.expect("list")[0].clone();
    s.enabled = false;
    source_repo.update(&s).await.expect("disable");
    let control = create_auto_source(&source_repo, "enabled-control", 3600).await;

    let scheduler = RefreshScheduler::new(
        source_repo.clone(),
        snapshot_repo.clone(),
        pool_repo.clone(),
        Arc::new(SqliteSourceRefreshJobRepository::new(db.pool.clone())),
        Arc::new(fetcher),
        Arc::new(StubGeoIp),
    )
    .tick_interval(Duration::from_millis(50));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    let handle = tokio::spawn(scheduler.run(shutdown));

    wait_for_completed_refresh(&db, control).await;
    stop_scheduler(shutdown_tx, handle).await;

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "only the enabled control may be refreshed"
    );
    assert!(
        snapshot_repo
            .list_for_source(s.id, 100)
            .await
            .expect("snapshots")
            .is_empty()
    );
    assert!(
        SqliteSourceRefreshJobRepository::new(db.pool.clone())
            .list_for_source(s.id, 100)
            .await
            .expect("jobs")
            .is_empty(),
        "disabled source cannot acquire a refresh job"
    );
}

/// SRC-003: Shutdown signal stops the scheduler cleanly.
#[tokio::test]
async fn scheduler_stops_on_shutdown() {
    let db = TestDb::new().await;
    let source_repo = Arc::new(SqliteSourceRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    ));
    let snapshot_repo = Arc::new(SqliteSourceSnapshotRepository::new(db.pool.clone()));
    let pool_repo = Arc::new(SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    ));
    let (fetcher, _calls) = CountingFetcher::new(TROJAN_URI.as_bytes().to_vec());

    create_auto_source(&source_repo, "auto-source", 1).await;

    let scheduler = RefreshScheduler::new(
        source_repo,
        snapshot_repo,
        pool_repo,
        Arc::new(SqliteSourceRefreshJobRepository::new(db.pool.clone())),
        Arc::new(fetcher),
        Arc::new(StubGeoIp),
    )
    .tick_interval(Duration::from_secs(60));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    let handle = tokio::spawn(scheduler.run(shutdown));

    stop_scheduler(shutdown_tx, handle).await;
}

#[path = "scheduler/shutdown.rs"]
mod shutdown;

#[path = "scheduler/start_shutdown.rs"]
mod start_shutdown;
