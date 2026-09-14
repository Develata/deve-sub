//! SRC-009: cancellation must reach the worker without prematurely releasing its lease.

use super::*;
use deve_sub_application::RefreshScheduler;
use deve_sub_application::source::{RefreshDeps, start_refresh_job};
use deve_sub_domain::SourceRefreshJobStatus;
use deve_sub_kernel::SourceId;
use std::time::Duration;
use tokio::sync::{Notify, oneshot};
use tokio::time::timeout;

struct GatedFetcher {
    entered: Notify,
    release: Notify,
}

#[async_trait]
impl SubscriptionFetcher for GatedFetcher {
    async fn fetch(&self, _url: &str, _etag: Option<&str>) -> Result<FetchResult, FetchError> {
        self.entered.notify_one();
        timeout(Duration::from_secs(10), self.release.notified())
            .await
            .expect("test must release fetch");
        Ok(FetchResult::Ok {
            body: TROJAN_LIST.as_bytes().to_vec(),
            etag: None,
            content_type: Some("text/plain".to_owned()),
        })
    }
}

#[tokio::test]
async fn src009_scheduled_cancel_prevents_publish_and_releases_registration() {
    let mut app = TestApp::new_with_fetcher(MockFetcher::error()).await;
    let fetcher = Arc::new(GatedFetcher {
        entered: Notify::new(),
        release: Notify::new(),
    });
    app.state.fetcher = fetcher.clone();
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", r#"{"name":"scheduled","source_type":"uri_list","url":"https://example.com/sub","auto_update":true,"update_interval_secs":3600,"keep_on_fail":true}"#),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_to_json(response).await;
    let source_id = SourceId::parse(created["source"]["id"].as_str().expect("id")).expect("id");
    let scheduler = RefreshScheduler::new(
        app.state.source_repo.clone(),
        app.state.snapshot_repo.clone(),
        app.state.pool_repo.clone(),
        app.state.refresh_job_repo.clone(),
        app.state.fetcher.clone(),
        app.state.geoip.clone(),
    )
    .tick_interval(Duration::from_millis(10))
    .cancel_flags(app.state.refresh_cancel_flags.clone());
    let (stop, stopped) = oneshot::channel();
    let worker = tokio::spawn(scheduler.run(async {
        let _ = stopped.await;
    }));
    timeout(Duration::from_secs(5), fetcher.entered.notified())
        .await
        .expect("fetch started");
    let job = app
        .state
        .refresh_job_repo
        .find_running_for_source(source_id)
        .await
        .expect("running job")
        .expect("job");
    let response = router
        .clone()
        .oneshot(with_cookie(
            post(&format!("/api/v1/sources/refresh-jobs/{}/cancel", job.id)),
            &cookie,
        ))
        .await
        .expect("cancel");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_to_json(response).await["cancelled"], true);

    // WHY: stop before releasing the upstream gate, so a second scheduler tick
    // cannot publish a new job and hide the outcome of the cancelled one.
    stop.send(()).expect("stop");
    fetcher.release.notify_one();
    timeout(Duration::from_secs(5), worker)
        .await
        .expect("worker stops")
        .expect("worker");
    assert!(
        app.state
            .snapshot_repo
            .find_active(source_id)
            .await
            .expect("snapshot")
            .is_none(),
        "cancelled scheduled job must not publish"
    );
    let finished = app
        .state
        .refresh_job_repo
        .find_by_id(job.id)
        .await
        .expect("job")
        .expect("job");
    assert_eq!(finished.status, SourceRefreshJobStatus::Cancelled);
    assert!(
        app.state
            .refresh_cancel_flags
            .lock()
            .expect("flags")
            .is_empty()
    );

    // A cancelled worker has actually stopped before a replacement acquires its lease.
    fetcher.release.notify_one();
    let response = router
        .clone()
        .oneshot(with_cookie(
            post(&format!("/api/v1/sources/{source_id}/refresh")),
            &cookie,
        ))
        .await
        .expect("replacement");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let replacement = body_to_json(response).await;
    let finished = poll_job(
        &router,
        &cookie,
        replacement["job_id"].as_str().expect("id"),
    )
    .await;
    assert_eq!(finished["status"], "completed");
    // The terminal row can become visible just before the worker drops its guard.
    app.state
        .job_supervisor
        .shutdown(Duration::from_secs(5))
        .await;
    assert!(
        app.state
            .refresh_cancel_flags
            .lock()
            .expect("flags")
            .is_empty()
    );
}

#[tokio::test]
async fn src009_unregistered_cancel_preserves_running_lease() {
    let app = TestApp::new_with_fetcher(MockFetcher::error()).await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let source_id = SourceId::parse(&create_source(&router, &cookie).await).expect("id");
    let deps = RefreshDeps {
        source_repo: app.state.source_repo.as_ref(),
        snapshot_repo: app.state.snapshot_repo.as_ref(),
        pool_repo: app.state.pool_repo.as_ref(),
        job_repo: app.state.refresh_job_repo.as_ref(),
        fetcher: app.state.fetcher.as_ref(),
        geoip: app.state.geoip.as_ref(),
    };
    let id = start_refresh_job(&deps, source_id).await.expect("start");
    let response = router
        .clone()
        .oneshot(with_cookie(
            post(&format!("/api/v1/sources/refresh-jobs/{id}/cancel")),
            &cookie,
        ))
        .await
        .expect("cancel");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_to_json(response).await["error"], "cancel_unavailable");
    let job = app
        .state
        .refresh_job_repo
        .find_by_id(id)
        .await
        .expect("job")
        .expect("job");
    assert_eq!(job.status, SourceRefreshJobStatus::Running);
    assert!(matches!(
        start_refresh_job(&deps, source_id).await,
        Err(deve_sub_application::SourceAppError::RefreshInProgress(_))
    ));
}
