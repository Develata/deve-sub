//! AUDIT-004: API authorization, scope validation, and receipt visibility.
use super::*;
use deve_sub_domain::AuditLog;
use deve_sub_kernel::Timestamp;

async fn json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), 100_000)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

#[tokio::test]
async fn audit_004_api_preview_confirm_receipt_and_replay() {
    let app = TestApp::new().await;
    let (router, cookie, actor) = setup_and_login(&app).await;
    let mut entry = AuditLog::new(None, "fixture.old", None, None, None);
    entry.created_at = Timestamp::now() - time::Duration::days(100);
    app.state
        .audit_log_repo
        .insert(&entry)
        .await
        .expect("old event");
    let preview = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/audit-logs/cleanup/preview",
            r#"{"keep_days":90}"#,
            &cookie,
        ))
        .await
        .expect("preview");
    assert_eq!(preview.status(), StatusCode::OK);
    let p = json(preview).await;
    assert_eq!(p["entry_ids"], serde_json::json!([entry.id.to_string()]));
    let confirm =
        serde_json::json!({ "before_unix_ms": p["before_unix_ms"], "entry_ids": p["entry_ids"] })
            .to_string();
    let cleaned = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/audit-logs/cleanup",
            &confirm,
            &cookie,
        ))
        .await
        .expect("cleanup");
    assert_eq!(cleaned.status(), StatusCode::OK);
    let result = json(cleaned).await;
    assert_eq!(result["deleted"], 1);
    let replay = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/audit-logs/cleanup",
            &confirm,
            &cookie,
        ))
        .await
        .expect("replay");
    assert_eq!(replay.status(), StatusCode::CONFLICT);
    let history = json(
        router
            .oneshot(get_with_cookie(
                "/api/v1/audit-logs?action=audit.cleanup",
                &cookie,
            ))
            .await
            .expect("history"),
    )
    .await;
    assert_eq!(history["entries"][0]["id"], result["receipt_id"]);
    assert_eq!(history["entries"][0]["actor_id"], actor);
    assert!(
        history["entries"][0]["details_json"]
            .as_str()
            .expect("details")
            .contains("manual")
    );
}

#[tokio::test]
async fn audit_004_cleanup_requires_admin_and_same_origin() {
    let app = TestApp::new().await;
    let (router, cookie, _) = setup_and_login(&app).await;
    router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/users",
            r#"{"username":"fixture-user","password":"Fixture-password123!","role":"user"}"#,
            &cookie,
        ))
        .await
        .expect("user");
    let login = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"fixture-user","password":"Fixture-password123!"}"#,
        ))
        .await
        .expect("login");
    let user_cookie = extract_cookie(&login).expect("cookie");
    for path in [
        "/api/v1/audit-logs/cleanup/preview",
        "/api/v1/audit-logs/cleanup",
    ] {
        for (credentials, status) in [
            ("", StatusCode::UNAUTHORIZED),
            (user_cookie.as_str(), StatusCode::FORBIDDEN),
        ] {
            let response = router
                .clone()
                .oneshot(post_with_cookie(path, "{}", credentials))
                .await
                .expect("guard");
            assert_eq!(response.status(), status);
        }
        let mut request = post_with_cookie(path, "{}", &cookie);
        request.headers_mut().insert(
            "origin",
            "https://untrusted.example".parse().expect("origin"),
        );
        request
            .headers_mut()
            .insert("host", "localhost".parse().expect("host"));
        assert_eq!(
            router
                .clone()
                .oneshot(request)
                .await
                .expect("csrf")
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        router
            .clone()
            .oneshot(get_with_cookie("/api/v1/audit-logs/policy", &user_cookie))
            .await
            .expect("policy guard")
            .status(),
        StatusCode::FORBIDDEN
    );
    let p = json(
        router
            .oneshot(get_with_cookie("/api/v1/audit-logs/policy", &cookie))
            .await
            .expect("policy"),
    )
    .await;
    assert_eq!(
        p,
        serde_json::json!({ "retention_days": 90, "batch_limit": 500 })
    );
}

#[tokio::test]
async fn audit_001_time_range_and_invalid_cleanup_inputs() {
    let app = TestApp::new().await;
    let (router, cookie, _) = setup_and_login(&app).await;
    for ms in [1_700_000_000_000, 1_700_000_001_000] {
        let mut entry = AuditLog::new(None, "fixture.range", None, None, None);
        entry.created_at = Timestamp::from_unix_ms(ms).expect("timestamp");
        app.state
            .audit_log_repo
            .insert(&entry)
            .await
            .expect("entry");
    }
    let response = router
        .clone()
        .oneshot(get_with_cookie(
            "/api/v1/audit-logs?since=2023-11-14T22:13:20Z&before=2023-11-14T22:13:21Z",
            &cookie,
        ))
        .await
        .expect("range");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json(response).await["entries"]
            .as_array()
            .expect("entries")
            .len(),
        1
    );
    for query in [
        "since=invalid",
        "since=2023-11-14T22:13:20.5Z",
        "since=2024-01-01T00:00:00Z&before=2023-01-01T00:00:00Z",
    ] {
        assert_eq!(
            router
                .clone()
                .oneshot(get_with_cookie(
                    &format!("/api/v1/audit-logs?{query}"),
                    &cookie
                ))
                .await
                .expect("invalid")
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let invalid = serde_json::json!({ "before_unix_ms": 0, "entry_ids": ["invalid"] }).to_string();
    assert_eq!(
        router
            .oneshot(post_with_cookie(
                "/api/v1/audit-logs/cleanup",
                &invalid,
                &cookie
            ))
            .await
            .expect("invalid id")
            .status(),
        StatusCode::BAD_REQUEST
    );
}
