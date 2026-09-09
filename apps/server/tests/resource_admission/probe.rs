use super::*;

#[tokio::test]
async fn saturated_supervisor_returns_503_and_terminates_probe_run() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let node_id = import_node(&router, &cookie, "trojan://TEST_PASSWORD@127.0.0.1:9#test").await;
    for _ in 0..64 {
        app.state
            .job_supervisor
            .spawn(std::future::pending())
            .expect("admit blocker");
    }
    let response = router
        .oneshot(with_cookie(
            post_json(
                "/api/v1/probe-runs",
                &serde_json::json!({"probe_type":"tcp_connect", "node_ids":[node_id]}).to_string(),
            ),
            &cookie,
        ))
        .await
        .expect("probe");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(app.state.cancelled_flags.lock().expect("flags").is_empty());
    assert_eq!(
        app.state
            .probe_run_repo
            .recover_crashed_runs()
            .await
            .expect("no active runs"),
        0
    );
    let db_path = app._dir.path().join("test.db");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", db_path.display()))
        .await
        .expect("pool");
    let cancelled: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM probe_runs WHERE status = 'X'")
        .fetch_one(&pool)
        .await
        .expect("cancelled run");
    assert_eq!(cancelled, 1);
    pool.close().await;
    app.state
        .job_supervisor
        .shutdown(std::time::Duration::ZERO)
        .await;
}
