//! Refresh configuration, lease and publication share durable boundaries.
use super::*;
use deve_sub_domain::{SourceConfigUpdate, SourceFilterRules, SourceRepository};

fn edit(s: &deve_sub_domain::Source) -> SourceConfigUpdate {
    SourceConfigUpdate {
        id: s.id,
        name: s.name.clone(),
        source_type: s.source_type,
        url: None,
        auto_update: false,
        update_interval_secs: 3600,
        enabled: true,
        keep_on_fail: true,
        filter_rules: None,
    }
}

fn body_fetcher() -> MockFetcher {
    MockFetcher::new(vec![MockResponse::Ok {
        body: TROJAN_URI_LIST.as_bytes().to_vec(),
        etag: Some("v1".into()),
        content_type: None,
    }])
}

struct Fixture {
    db: TestDb,
    source: deve_sub_domain::Source,
    repo: SqliteSourceRepository,
    snaps: SqliteSourceSnapshotRepository,
    nodes: SqliteNodePoolRepository,
    jobs: SqliteSourceRefreshJobRepository,
}
impl Fixture {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let repo = SqliteSourceRepository::new_with_key(db.pool.clone(), db.master_key.clone());
        let snaps = SqliteSourceSnapshotRepository::new(db.pool.clone());
        let nodes = SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
        let jobs = SqliteSourceRefreshJobRepository::new(db.pool.clone());
        let source = create_source(&repo, "lease-fixture").await;
        Self {
            db,
            source,
            repo,
            snaps,
            nodes,
            jobs,
        }
    }
    fn deps<'a>(&'a self, fetcher: &'a dyn SubscriptionFetcher) -> RefreshDeps<'a> {
        RefreshDeps {
            source_repo: &self.repo,
            snapshot_repo: &self.snaps,
            pool_repo: &self.nodes,
            job_repo: &self.jobs,
            fetcher,
            geoip: &StubGeoIp,
        }
    }
    async fn refresh(&self) -> Result<RefreshResult, source::SourceAppError> {
        run_refresh(
            &self.repo,
            &self.snaps,
            &self.nodes,
            &self.db.pool,
            &body_fetcher(),
            &StubGeoIp,
            self.source.id,
        )
        .await
    }
}

#[tokio::test]
async fn config_edits_clear_validator_and_preserve_last_snapshot() {
    let f = Fixture::new().await;
    f.refresh().await.expect("first");
    let mut update = edit(&f.source);
    update.filter_rules = Some(SourceFilterRules {
        exclude_protocols: vec!["trojan".into()],
        ..Default::default()
    });
    f.repo.update_config(&update).await.expect("edit");
    let snapshot = f
        .snaps
        .find_active(f.source.id)
        .await
        .expect("get")
        .expect("last good");
    assert_eq!(snapshot.etag, None);
    assert_eq!(snapshot.node_count, 2);
    // A full unchanged body is parsed again, so the new filter applies.
    assert!(matches!(
        f.refresh().await,
        Err(source::SourceAppError::ZeroNodes)
    ));
    assert_eq!(
        f.snaps
            .find_active(f.source.id)
            .await
            .expect("get")
            .expect("last good")
            .version,
        1
    );
}

#[tokio::test]
async fn running_refresh_excludes_configuration_edits_then_releases_lease() {
    let f = Fixture::new().await;
    let fetcher = body_fetcher();
    let deps = f.deps(&fetcher);
    let job = start_refresh_job(&deps, f.source.id).await.expect("start");
    let error = source::update_source(&f.repo, edit(&f.source))
        .await
        .expect_err("running");
    assert!(matches!(
        error,
        source::SourceAppError::RefreshInProgress(_)
    ));
    assert!(matches!(
        f.repo.update(&f.source).await,
        Err(deve_sub_domain::SourceError::RefreshInProgress(_))
    ));
    execute_refresh_job(
        &deps,
        job,
        f.source.id,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .await
    .expect("completed");
    let status: String = sqlx::query_scalar("SELECT status FROM source_refresh_jobs WHERE id = ?")
        .bind(job.to_string())
        .fetch_one(&f.db.pool)
        .await
        .expect("status");
    assert_eq!(status, "C");
    f.repo
        .update_config(&edit(&f.source))
        .await
        .expect("edit after commit");
}

struct PausedBody {
    entered: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}
#[async_trait]
impl SubscriptionFetcher for PausedBody {
    async fn fetch(&self, url: &str, _etag: Option<&str>) -> Result<FetchResult, FetchError> {
        assert_eq!(url, "https://example.com/sub");
        self.entered.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(5), self.resume.notified())
            .await
            .expect("resume old fetch");
        Ok(FetchResult::Ok {
            body: TROJAN_URI_LIST.as_bytes().to_vec(),
            etag: Some("old-config".into()),
            content_type: None,
        })
    }
}

#[tokio::test]
async fn reclaimed_runner_cannot_publish_after_configuration_edit() {
    let f = Fixture::new().await;
    let fetcher = PausedBody {
        entered: Default::default(),
        resume: Default::default(),
    };
    let deps = f.deps(&fetcher);
    let job = start_refresh_job(&deps, f.source.id).await.expect("start");
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let execute = execute_refresh_job(&deps, job, f.source.id, &cancelled);
    let edit_during_fetch = async {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            fetcher.entered.notified(),
        )
        .await
        .expect("fetch started");
        assert!(matches!(
            f.repo.update_config(&edit(&f.source)).await,
            Err(deve_sub_domain::SourceError::RefreshInProgress(_))
        ));
        sqlx::query("UPDATE source_refresh_jobs SET status = 'F' WHERE id = ?")
            .bind(job.to_string())
            .execute(&f.db.pool)
            .await
            .expect("reclaim");
        let mut update = edit(&f.source);
        update.url = Some("https://new.example/sub".into());
        f.repo.update_config(&update).await.expect("edit");
        fetcher.resume.notify_one();
    };
    let (result, ()) = tokio::join!(execute, edit_during_fetch);
    assert!(result.is_err());
    assert!(
        f.snaps
            .find_active(f.source.id)
            .await
            .expect("get")
            .is_none()
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes")
        .fetch_one(&f.db.pool)
        .await
        .expect("count");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn terminal_write_failure_rolls_back_publication() {
    let f = Fixture::new().await;
    f.refresh().await.expect("first");
    sqlx::query("CREATE TRIGGER fail_completion BEFORE UPDATE OF status ON source_refresh_jobs WHEN NEW.status = 'C' BEGIN SELECT RAISE(FAIL, 'injected terminal write failure'); END")
        .execute(&f.db.pool).await.expect("trigger");
    assert!(f.refresh().await.is_err());
    let active = f
        .snaps
        .find_active(f.source.id)
        .await
        .expect("get")
        .expect("old");
    assert_eq!(active.version, 1);
    let states: Vec<String> =
        sqlx::query_scalar("SELECT status FROM source_refresh_jobs ORDER BY id")
            .fetch_all(&f.db.pool)
            .await
            .expect("states");
    assert_eq!(states, vec!["C", "F"]);
}

#[tokio::test]
async fn publication_has_no_fallible_running_phase_write_after_commit() {
    let f = Fixture::new().await;
    sqlx::query("CREATE TRIGGER fail_phase BEFORE UPDATE OF phase ON source_refresh_jobs WHEN NEW.phase = 'publishing' AND NEW.status = 'R' BEGIN SELECT RAISE(FAIL, 'injected phase failure'); END")
        .execute(&f.db.pool).await.expect("trigger");
    f.refresh().await.expect("atomic publication");
}

#[tokio::test]
async fn reclaimed_not_modified_run_cannot_touch_snapshot() {
    let f = Fixture::new().await;
    f.refresh().await.expect("first");
    let before = f
        .snaps
        .find_active(f.source.id)
        .await
        .expect("get")
        .expect("exists");
    let fetcher = MockFetcher::new(vec![MockResponse::NotModified]);
    let deps = f.deps(&fetcher);
    let job = start_refresh_job(&deps, f.source.id).await.expect("start");
    sqlx::query("UPDATE source_refresh_jobs SET status = 'F' WHERE id = ?")
        .bind(job.to_string())
        .execute(&f.db.pool)
        .await
        .expect("reclaim");
    assert!(
        execute_refresh_job(
            &deps,
            job,
            f.source.id,
            &std::sync::atomic::AtomicBool::new(false)
        )
        .await
        .is_err()
    );
    let after = f
        .snaps
        .find_active(f.source.id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(before.fetched_at, after.fetched_at);
}
