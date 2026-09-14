//! SRC-003: a shutdown during a tick drains admitted work without starting queued sources.

use super::*;
use deve_sub_application::CancellationFlags;
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio::time::timeout;

struct GatedFetcher {
    entered: mpsc::UnboundedSender<()>,
    release: Arc<Semaphore>,
    calls: Arc<AtomicU32>,
}

#[async_trait]
impl SubscriptionFetcher for GatedFetcher {
    async fn fetch(&self, _url: &str, _etag: Option<&str>) -> Result<FetchResult, FetchError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.send(()).expect("test receiver");
        timeout(Duration::from_secs(5), self.release.acquire())
            .await
            .expect("test releases fetch")
            .expect("semaphore")
            .forget();
        Ok(FetchResult::Ok {
            body: TROJAN_URI.as_bytes().to_vec(),
            etag: None,
            content_type: Some("text/plain".to_owned()),
        })
    }
}

async fn stop_during_batch(concurrency: usize, cancel_running: bool) {
    let db = TestDb::new().await;
    let source_repo = Arc::new(SqliteSourceRepository::new_with_key(
        db.pool.clone(),
        db.master_key.clone(),
    ));
    let snapshot_repo = Arc::new(SqliteSourceSnapshotRepository::new(db.pool.clone()));
    let pool_repo = Arc::new(SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        db.master_key.clone(),
    ));
    let count = 9;
    for index in 0..count {
        create_auto_source(&source_repo, &format!("queued-{index}"), 3600).await;
    }
    let (entered, mut events) = mpsc::unbounded_channel();
    let release = Arc::new(Semaphore::new(0));
    let calls = Arc::new(AtomicU32::new(0));
    let flags = CancellationFlags::default();
    let scheduler = RefreshScheduler::new(
        source_repo,
        snapshot_repo,
        pool_repo,
        Arc::new(SqliteSourceRefreshJobRepository::new(db.pool.clone())),
        Arc::new(GatedFetcher {
            entered,
            release: release.clone(),
            calls: calls.clone(),
        }),
        Arc::new(StubGeoIp),
    )
    .max_concurrency(concurrency)
    .tick_interval(Duration::from_millis(1))
    .cancel_flags(flags.clone());
    let (stop, stopped) = oneshot::channel();
    let worker = tokio::spawn(scheduler.run(async {
        let _ = stopped.await;
    }));
    for _ in 0..concurrency {
        timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("fetch starts")
            .expect("event");
    }
    assert_eq!(flags.lock().expect("flags").len(), concurrency);
    stop.send(()).expect("shutdown");
    if cancel_running {
        // Production shutdown signals the registrations that exist at shutdown.
        // Later queued jobs must not escape that snapshot of active workers.
        for flag in flags.lock().expect("flags").values() {
            flag.store(true, Ordering::Relaxed);
        }
    }
    release.add_permits(count);
    timeout(Duration::from_secs(5), worker)
        .await
        .expect("bounded drain")
        .expect("worker");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        concurrency as u32,
        "shutdown must not fetch queued sources after the active group exits"
    );
    assert!(flags.lock().expect("flags").is_empty());
    let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM source_refresh_jobs")
        .fetch_one(&db.pool)
        .await
        .expect("jobs");
    assert_eq!(
        jobs, concurrency as i64,
        "queued sources must not acquire leases"
    );
    let unfinished: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM source_refresh_jobs WHERE status IN ('P', 'R')")
            .fetch_one(&db.pool)
            .await
            .expect("unfinished jobs");
    assert_eq!(unfinished, 0, "drain must finish durable job state");
    let snapshots: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM source_snapshots")
        .fetch_one(&db.pool)
        .await
        .expect("snapshots");
    assert_eq!(
        snapshots,
        if cancel_running {
            0
        } else {
            concurrency as i64
        }
    );
}

#[tokio::test]
async fn src003_shutdown_drains_active_group_without_starting_queued_sources() {
    for concurrency in [1, 4] {
        stop_during_batch(concurrency, false).await;
    }
}

#[tokio::test]
async fn src003_shutdown_cancel_does_not_escape_to_queued_sources() {
    stop_during_batch(4, true).await;
}
