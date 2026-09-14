//! SRC-003: shutdown cannot drop a durable start between lease and registration.

use super::*;
use deve_sub_application::CancellationFlags;
use deve_sub_domain::{RefreshPhase, SourceError, SourceRefreshJob, SourceRefreshJobRepository};
use deve_sub_kernel::{SourceId, SourceRefreshJobId, Timestamp};
use tokio::sync::{Notify, oneshot};
use tokio::time::timeout;

struct GatedStart {
    inner: SqliteSourceRefreshJobRepository,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl SourceRefreshJobRepository for GatedStart {
    async fn create(&self, job: &SourceRefreshJob) -> Result<(), SourceError> {
        self.inner.create(job).await
    }
    async fn find_by_id(
        &self,
        id: SourceRefreshJobId,
    ) -> Result<Option<SourceRefreshJob>, SourceError> {
        self.inner.find_by_id(id).await
    }
    async fn find_running_for_source(
        &self,
        id: SourceId,
    ) -> Result<Option<SourceRefreshJob>, SourceError> {
        self.inner.find_running_for_source(id).await
    }
    async fn mark_running(&self, id: SourceRefreshJobId) -> Result<(), SourceError> {
        self.inner.mark_running(id).await?;
        self.entered.notify_one();
        timeout(Duration::from_secs(5), self.release.notified())
            .await
            .expect("release durable start");
        Ok(())
    }
    async fn update_phase(
        &self,
        id: SourceRefreshJobId,
        phase: RefreshPhase,
    ) -> Result<(), SourceError> {
        self.inner.update_phase(id, phase).await
    }
    async fn mark_completed(
        &self,
        id: SourceRefreshJobId,
        new: u64,
        duplicate: u64,
        reactivated: u64,
        missing: u64,
        not_modified: bool,
    ) -> Result<(), SourceError> {
        self.inner
            .mark_completed(id, new, duplicate, reactivated, missing, not_modified)
            .await
    }
    async fn mark_failed(&self, id: SourceRefreshJobId, message: &str) -> Result<(), SourceError> {
        self.inner.mark_failed(id, message).await
    }
    async fn mark_cancelled(&self, id: SourceRefreshJobId) -> Result<(), SourceError> {
        self.inner.mark_cancelled(id).await
    }
    async fn delete(&self, id: SourceRefreshJobId) -> Result<(), SourceError> {
        self.inner.delete(id).await
    }
    async fn list_for_source(
        &self,
        id: SourceId,
        limit: u32,
    ) -> Result<Vec<SourceRefreshJob>, SourceError> {
        self.inner.list_for_source(id, limit).await
    }
    async fn recover_crashed_jobs(&self) -> Result<u64, SourceError> {
        self.inner.recover_crashed_jobs().await
    }
    async fn recover_stale_jobs(
        &self,
        cutoff: Timestamp,
        reason: &str,
    ) -> Result<u64, SourceError> {
        self.inner.recover_stale_jobs(cutoff, reason).await
    }
}

#[tokio::test]
async fn src003_shutdown_during_durable_start_cancels_before_fetch() {
    let db = TestDb::new().await;
    let sources = Arc::new(SqliteSourceRepository::new_with_key(
        db.pool.clone(),
        db.master_key.clone(),
    ));
    create_auto_source(&sources, "start-window", 3600).await;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let flags = CancellationFlags::default();
    let (fetcher, calls) = CountingFetcher::new(TROJAN_URI.as_bytes().to_vec());
    let scheduler = RefreshScheduler::new(
        sources,
        Arc::new(SqliteSourceSnapshotRepository::new(db.pool.clone())),
        Arc::new(SqliteNodePoolRepository::new_with_key(
            db.pool.clone(),
            db.master_key.clone(),
        )),
        Arc::new(GatedStart {
            inner: SqliteSourceRefreshJobRepository::new(db.pool.clone()),
            entered: entered.clone(),
            release: release.clone(),
        }),
        Arc::new(fetcher),
        Arc::new(StubGeoIp),
    )
    .tick_interval(Duration::from_millis(1))
    .cancel_flags(flags.clone());
    let (stop, stopped) = oneshot::channel();
    let worker = tokio::spawn(scheduler.run(async {
        let _ = stopped.await;
    }));
    timeout(Duration::from_secs(5), entered.notified())
        .await
        .expect("durable lease acquired");
    let status: String = sqlx::query_scalar("SELECT status FROM source_refresh_jobs")
        .fetch_one(&db.pool)
        .await
        .expect("durable status");
    assert_eq!(status, "R");
    assert!(
        flags.lock().expect("flags").is_empty(),
        "start has not registered yet"
    );
    stop.send(()).expect("shutdown");
    release.notify_one();
    timeout(Duration::from_secs(5), worker)
        .await
        .expect("drain start")
        .expect("worker");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "late registration must inherit shutdown"
    );
    let status: String = sqlx::query_scalar("SELECT status FROM source_refresh_jobs")
        .fetch_one(&db.pool)
        .await
        .expect("final durable status");
    assert_eq!(
        status, "X",
        "normal shutdown must not abandon a Running lease"
    );
    assert!(flags.lock().expect("flags").is_empty());
}
