#![allow(clippy::expect_used)]

//! Regression tests for stale `Running` refresh job recovery (P0-10).
//!
//! A process crash mid-refresh leaves the job row in `Running`. Because the
//! per-source lease is a partial UNIQUE index
//! `(source_id) WHERE status = 'R'`, a stuck Running row blocks all future
//! refreshes for that source indefinitely. These tests verify both recovery
//! paths:
//!
//! - `recover_crashed_jobs`: blanket startup sweep (Pending + Running → Failed)
//! - `recover_stale_jobs`: age-based tick sweep (Running older than cutoff → Failed)

use deve_sub_domain::{
    RefreshPhase, Source, SourceRefreshJob, SourceRefreshJobRepository, SourceRefreshJobStatus,
    SourceRepository, SourceType,
};
use deve_sub_kernel::{SourceId, SourceRefreshJobId, Timestamp};
use deve_sub_storage_sqlite::{SqliteSourceRefreshJobRepository, SqliteSourceRepository};

struct TestDb {
    job_repo: SqliteSourceRefreshJobRepository,
    source_repo: SqliteSourceRepository,
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
            job_repo: SqliteSourceRefreshJobRepository::new(pool.clone()),
            source_repo: SqliteSourceRepository::new(pool),
            _dir: dir,
        }
    }

    async fn insert_source(&self, name: &str) -> SourceId {
        let mut source = Source::new(
            name,
            SourceType::UriList,
            "https://example.com/sub".to_owned(),
        );
        source.id = SourceId::new();
        self.source_repo
            .create(&source)
            .await
            .expect("create source");
        source.id
    }

    async fn insert_running_job(&self, source_id: SourceId) -> SourceRefreshJobId {
        let job = SourceRefreshJob {
            id: SourceRefreshJobId::new(),
            source_id,
            status: SourceRefreshJobStatus::Pending,
            phase: RefreshPhase::Idle,
            started_at: Timestamp::now(),
            finished_at: None,
            error_message: None,
            new_nodes: 0,
            duplicate_nodes: 0,
            reactivated_nodes: 0,
            missing_nodes: 0,
            not_modified: false,
        };
        self.job_repo.create(&job).await.expect("create job");
        self.job_repo
            .mark_running(job.id)
            .await
            .expect("mark running");
        job.id
    }

    /// Insert a Running job with `started_at` set to a past timestamp,
    /// simulating a job that has been running longer than the lease timeout.
    async fn insert_stale_running_job(
        &self,
        source_id: SourceId,
        started_at: Timestamp,
    ) -> SourceRefreshJobId {
        let job = SourceRefreshJob {
            id: SourceRefreshJobId::new(),
            source_id,
            status: SourceRefreshJobStatus::Pending,
            phase: RefreshPhase::Idle,
            started_at,
            finished_at: None,
            error_message: None,
            new_nodes: 0,
            duplicate_nodes: 0,
            reactivated_nodes: 0,
            missing_nodes: 0,
            not_modified: false,
        };
        self.job_repo.create(&job).await.expect("create job");
        self.job_repo
            .mark_running(job.id)
            .await
            .expect("mark running");
        job.id
    }
}

#[tokio::test]
async fn recover_crashed_jobs_releases_stale_lease() {
    let db = TestDb::new().await;
    let source_id = db.insert_source("source-a").await;
    let job_id = db.insert_running_job(source_id).await;

    // The stuck Running job holds the lease. Creating a second Pending job
    // is allowed (the partial unique index only constrains status = 'R'),
    // but transitioning it to Running must be rejected.
    let second = SourceRefreshJob {
        id: SourceRefreshJobId::new(),
        source_id,
        status: SourceRefreshJobStatus::Pending,
        phase: RefreshPhase::Idle,
        started_at: Timestamp::now(),
        finished_at: None,
        error_message: None,
        new_nodes: 0,
        duplicate_nodes: 0,
        reactivated_nodes: 0,
        missing_nodes: 0,
        not_modified: false,
    };
    db.job_repo
        .create(&second)
        .await
        .expect("Pending job can coexist with a Running job");
    let err = db.job_repo.mark_running(second.id).await;
    assert!(
        err.is_err(),
        "mark_running should be rejected while lease is held"
    );

    // Recovery sweep: mark all Pending/Running as Failed. Both the stuck
    // Running job AND the second Pending job (whose mark_running was
    // rejected) are recovered.
    let recovered = db.job_repo.recover_crashed_jobs().await.expect("recover");
    assert_eq!(recovered, 2, "stuck Running + orphaned Pending = 2");

    // The recovered job is now Failed.
    let job = db
        .job_repo
        .find_by_id(job_id)
        .await
        .expect("find")
        .expect("job exists");
    assert_eq!(job.status, SourceRefreshJobStatus::Failed);
    assert!(job.finished_at.is_some(), "finished_at must be set");
    assert!(
        job.error_message
            .as_deref()
            .is_some_and(|m| m.contains("crashed")),
        "error_message should mention crash: {job:?}"
    );

    // The lease is released: a new job can be created AND transitioned to Running.
    let new_job = SourceRefreshJob {
        id: SourceRefreshJobId::new(),
        source_id,
        status: SourceRefreshJobStatus::Pending,
        phase: RefreshPhase::Idle,
        started_at: Timestamp::now(),
        finished_at: None,
        error_message: None,
        new_nodes: 0,
        duplicate_nodes: 0,
        reactivated_nodes: 0,
        missing_nodes: 0,
        not_modified: false,
    };
    db.job_repo
        .create(&new_job)
        .await
        .expect("create should succeed after recovery");
    db.job_repo
        .mark_running(new_job.id)
        .await
        .expect("mark_running should succeed after recovery (lease released)");
}

#[tokio::test]
async fn recover_crashed_jobs_handles_multiple_sources() {
    let db = TestDb::new().await;
    let s1 = db.insert_source("multi-a").await;
    let s2 = db.insert_source("multi-b").await;
    let _s3 = db.insert_source("multi-c").await;

    db.insert_running_job(s1).await;
    db.insert_running_job(s2).await;
    // s3 has no job — should not affect the count.

    let recovered = db.job_repo.recover_crashed_jobs().await.expect("recover");
    assert_eq!(recovered, 2, "two stuck jobs across two sources");

    // Running a second sweep must find zero — idempotent.
    let recovered_again = db.job_repo.recover_crashed_jobs().await.expect("recover");
    assert_eq!(recovered_again, 0, "second sweep finds nothing");
}

#[tokio::test]
async fn recover_crashed_jobs_preserves_terminal_jobs() {
    let db = TestDb::new().await;
    let source_id = db.insert_source("preserve-term").await;

    let mut completed = SourceRefreshJob {
        id: SourceRefreshJobId::new(),
        source_id,
        status: SourceRefreshJobStatus::Pending,
        phase: RefreshPhase::Idle,
        started_at: Timestamp::now(),
        finished_at: None,
        error_message: None,
        new_nodes: 5,
        duplicate_nodes: 0,
        reactivated_nodes: 0,
        missing_nodes: 0,
        not_modified: false,
    };
    db.job_repo.create(&completed).await.expect("create");
    db.job_repo.mark_running(completed.id).await.expect("run");
    db.job_repo
        .mark_completed(completed.id, 5, 0, 0, 0, false)
        .await
        .expect("complete");
    completed.status = SourceRefreshJobStatus::Completed;

    // A separate stuck Running job.
    db.insert_running_job(source_id).await;

    let recovered = db.job_repo.recover_crashed_jobs().await.expect("recover");
    assert_eq!(recovered, 1, "only the Running job is recovered");

    // The Completed job is untouched.
    let job = db
        .job_repo
        .find_by_id(completed.id)
        .await
        .expect("find")
        .expect("exists");
    assert_eq!(job.status, SourceRefreshJobStatus::Completed);
    assert_eq!(job.new_nodes, 5, "completed counts must be preserved");
}

#[tokio::test]
async fn recover_stale_jobs_only_touches_old_running() {
    let db = TestDb::new().await;
    let source_id = db.insert_source("stale-age").await;

    // A job started 30 minutes ago — older than a 10-minute cutoff.
    let old_started = Timestamp::now() - time::Duration::seconds(i64::from(30 * 60));
    let old_job = db.insert_stale_running_job(source_id, old_started).await;

    let source_id_2 = db.insert_source("stale-fresh").await;
    // A job started just now — younger than the cutoff, must survive.
    let fresh_job = db.insert_running_job(source_id_2).await;

    let cutoff = Timestamp::now() - time::Duration::seconds(10 * 60);
    let recovered = db
        .job_repo
        .recover_stale_jobs(cutoff, "lease timed out")
        .await
        .expect("recover stale");
    assert_eq!(recovered, 1, "only the old job is recovered");

    let old = db
        .job_repo
        .find_by_id(old_job)
        .await
        .expect("find")
        .expect("exists");
    assert_eq!(old.status, SourceRefreshJobStatus::Failed);

    let fresh = db
        .job_repo
        .find_by_id(fresh_job)
        .await
        .expect("find")
        .expect("exists");
    assert_eq!(
        fresh.status,
        SourceRefreshJobStatus::Running,
        "fresh job must keep its lease"
    );
}

#[tokio::test]
async fn recover_stale_jobs_releases_lease_for_new_refresh() {
    let db = TestDb::new().await;
    let source_id = db.insert_source("stale-release").await;

    let old_started = Timestamp::now() - time::Duration::seconds(i64::from(20 * 60));
    db.insert_stale_running_job(source_id, old_started).await;

    // Lease is held: creating a Pending job succeeds, but mark_running fails.
    let blocked = SourceRefreshJob {
        id: SourceRefreshJobId::new(),
        source_id,
        status: SourceRefreshJobStatus::Pending,
        phase: RefreshPhase::Idle,
        started_at: Timestamp::now(),
        finished_at: None,
        error_message: None,
        new_nodes: 0,
        duplicate_nodes: 0,
        reactivated_nodes: 0,
        missing_nodes: 0,
        not_modified: false,
    };
    db.job_repo
        .create(&blocked)
        .await
        .expect("Pending job can coexist with Running");
    assert!(
        db.job_repo.mark_running(blocked.id).await.is_err(),
        "mark_running should be rejected while lease is held"
    );

    let cutoff = Timestamp::now() - time::Duration::seconds(10 * 60);
    db.job_repo
        .recover_stale_jobs(cutoff, "lease timed out")
        .await
        .expect("recover");

    // Lease released: the pending job can now transition to Running.
    db.job_repo
        .mark_running(blocked.id)
        .await
        .expect("mark_running should succeed after stale recovery");
}
