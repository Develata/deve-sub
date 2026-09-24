//! SRC-001: masked source responses cannot be reused as secret update values.

use super::*;

const ORIGINAL_URL: &str = "https://user:fixture-pass@source.example/private/sub?token=fixture-old";
const REPLACEMENT_URL: &str = "https://source.example/new-sub?token=fixture-new";

fn update_body(url: Option<&str>) -> serde_json::Value {
    let mut value = serde_json::json!({
        "name": "renamed", "source_type": "auto", "auto_update": false,
        "update_interval_secs": 1800, "enabled": true, "keep_on_fail": true
    });
    if let Some(url) = url {
        value["url"] = url.into();
    }
    value
}

#[tokio::test]
async fn src001_edit_preserves_omitted_secret_and_accepts_explicit_replacement() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let create =
        serde_json::json!({"name": "original", "source_type": "auto", "url": ORIGINAL_URL});
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", &create.to_string()),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_to_json(response).await;
    let id = created["source"]["id"].as_str().expect("source ID");
    let source_id = deve_sub_kernel::SourceId::parse(id).expect("ID");
    let uri = format!("/api/v1/sources/{id}");
    assert!(!created.to_string().contains("fixture-old"));

    for (value, expected_url) in [
        (update_body(None), ORIGINAL_URL),
        (update_body(Some(REPLACEMENT_URL)), REPLACEMENT_URL),
        (
            {
                let mut value = update_body(None);
                value["url"] = serde_json::Value::Null;
                value
            },
            REPLACEMENT_URL,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(with_cookie(put_json(&uri, &value.to_string()), &cookie))
            .await
            .expect("update");
        assert_eq!(response.status(), StatusCode::OK);
        let updated = body_to_json(response).await;
        assert_eq!(updated["source"]["name"], "renamed");
        assert!(!updated.to_string().contains("fixture-"));
        let stored = app
            .state
            .source_repo
            .find_by_id(source_id)
            .await
            .expect("stored source")
            .expect("exists");
        assert!(
            stored.url == expected_url,
            "stored URL must retain the requested secret semantics"
        );
    }

    let response = router
        .oneshot(with_cookie(
            put_json(&uri, &update_body(Some("")).to_string()),
            &cookie,
        ))
        .await
        .expect("empty replacement");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let stored = app
        .state
        .source_repo
        .find_by_id(source_id)
        .await
        .expect("stored source")
        .expect("exists");
    assert!(
        stored.url == REPLACEMENT_URL,
        "rejected update must retain URL"
    );
}

#[tokio::test]
async fn src001_running_refresh_rejects_edit_with_retryable_conflict() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let created = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", VALID_SOURCE_BODY),
            &cookie,
        ))
        .await
        .expect("create");
    let created = body_to_json(created).await;
    let id = created["source"]["id"].as_str().expect("ID");
    let source_id = deve_sub_kernel::SourceId::parse(id).expect("source ID");
    let job = deve_sub_domain::SourceRefreshJob {
        id: deve_sub_kernel::SourceRefreshJobId::new(),
        source_id,
        status: deve_sub_domain::SourceRefreshJobStatus::Pending,
        phase: deve_sub_domain::RefreshPhase::Idle,
        started_at: deve_sub_kernel::Timestamp::now(),
        finished_at: None,
        error_message: None,
        new_nodes: 0,
        duplicate_nodes: 0,
        reactivated_nodes: 0,
        missing_nodes: 0,
        not_modified: false,
    };
    app.state.refresh_job_repo.create(&job).await.expect("job");
    app.state
        .refresh_job_repo
        .mark_running(job.id)
        .await
        .expect("lease");
    let uri = format!("/api/v1/sources/{id}");
    let response = router
        .clone()
        .oneshot(with_cookie(
            put_json(&uri, &update_body(None).to_string()),
            &cookie,
        ))
        .await
        .expect("update");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = body_to_json(response).await;
    assert_eq!(body["error"], "refresh_in_progress");
    let stored = app
        .state
        .source_repo
        .find_by_id(source_id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(stored.name, "my-sub");
    app.state
        .refresh_job_repo
        .mark_cancelled(job.id)
        .await
        .expect("finish");
    let response = router
        .oneshot(with_cookie(
            put_json(&uri, &update_body(None).to_string()),
            &cookie,
        ))
        .await
        .expect("retry");
    assert_eq!(response.status(), StatusCode::OK);
}
