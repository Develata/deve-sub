use super::*;

#[tokio::test]
async fn saturated_supervisor_returns_503_and_releases_refresh_lease() {
    let app = TestApp::new_with_fetcher(MockFetcher::ok(TROJAN_LIST)).await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let source_id = create_source(&router, &cookie).await;
    for _ in 0..64 {
        app.state
            .job_supervisor
            .spawn(std::future::pending())
            .expect("admit blocker");
    }
    let response = router
        .oneshot(with_cookie(
            post(&format!("/api/v1/sources/{source_id}/refresh")),
            &cookie,
        ))
        .await
        .expect("refresh");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let jobs = app
        .state
        .refresh_job_repo
        .list_for_source(
            deve_sub_kernel::SourceId::parse(&source_id).expect("id"),
            10,
        )
        .await
        .expect("jobs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        jobs[0].status,
        deve_sub_domain::SourceRefreshJobStatus::Failed
    );
    assert!(
        app.state
            .refresh_cancel_flags
            .lock()
            .expect("flags")
            .is_empty()
    );
    assert_eq!(
        app.state
            .refresh_job_repo
            .recover_crashed_jobs()
            .await
            .expect("no leases"),
        0
    );
    app.state
        .job_supervisor
        .shutdown(std::time::Duration::ZERO)
        .await;
}
