use super::*;
use deve_sub_domain::SourceRepository;

struct PausedFailure {
    entered: tokio::sync::Notify,
    finish: tokio::sync::Notify,
}

#[async_trait]
impl SubscriptionFetcher for PausedFailure {
    async fn fetch(&self, _url: &str, _etag: Option<&str>) -> Result<FetchResult, FetchError> {
        self.entered.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(5), self.finish.notified())
            .await
            .expect("release fetch");
        Err(FetchError::Timeout(1))
    }
}

#[tokio::test]
async fn refresh_failure_preserves_concurrent_source_edits_and_current_policy() {
    for keep_on_fail in [true, false] {
        let db = TestDb::new().await;
        let repo = SqliteSourceRepository::new_with_key(db.pool.clone(), db.master_key.clone());
        let snapshots = SqliteSourceSnapshotRepository::new(db.pool.clone());
        let nodes = SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone());
        let mut original = create_source(&repo, "original").await;
        original.keep_on_fail = false;
        repo.update(&original).await.expect("old policy");
        let fetcher = PausedFailure {
            entered: tokio::sync::Notify::new(),
            finish: tokio::sync::Notify::new(),
        };
        let refresh = run_refresh(
            &repo,
            &snapshots,
            &nodes,
            &db.pool,
            &fetcher,
            &StubGeoIp,
            original.id,
        );
        let edit = async {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                fetcher.entered.notified(),
            )
            .await
            .expect("fetch entered");
            let mut changed = original.clone();
            changed.name = "edited during fetch".into();
            changed.url = "https://new.example.com/sub".into();
            changed.update_interval_secs = 7200;
            changed.keep_on_fail = keep_on_fail;
            repo.update(&changed).await.expect("concurrent edit");
            fetcher.finish.notify_one();
        };
        let (result, ()) = tokio::join!(refresh, edit);
        assert!(result.is_err());
        let after = source::get_source(&repo, original.id)
            .await
            .expect("read")
            .expect("source");
        assert_eq!(after.name, "edited during fetch");
        assert_eq!(after.url, "https://new.example.com/sub");
        assert_eq!(after.update_interval_secs, 7200);
        assert_eq!(after.keep_on_fail, keep_on_fail);
        assert_eq!(after.enabled, keep_on_fail);
    }
}
