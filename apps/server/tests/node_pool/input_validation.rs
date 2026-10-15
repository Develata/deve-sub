use super::*;

/// NODE-001: blank imports must agree with the Web form and have no write effects.
#[tokio::test]
async fn whitespace_import_returns_400_without_changing_pool_revision() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let before = app
        .state
        .pool_meta_repo
        .get_revision()
        .await
        .expect("revision");

    for content in [" ", "\t\r\n", "\u{2003}\u{3000}"] {
        for source_type in ["uri_list", "auto", "base64", "mihomo_yaml"] {
            let body = serde_json::json!({ "source_type": source_type, "content": content });
            let response = router
                .clone()
                .oneshot(with_cookie(
                    post_json("/api/v1/nodes/import", &body.to_string()),
                    &cookie,
                ))
                .await
                .expect("blank import");
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{source_type}");
            let json = body_to_json(response).await;
            assert_eq!(json["error"], "invalid_input");
            assert_eq!(
                app.state
                    .pool_meta_repo
                    .get_revision()
                    .await
                    .expect("revision"),
                before
            );
        }
    }
    let response = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/nodes"), &cookie))
        .await
        .expect("list");
    assert!(
        body_to_json(response).await["nodes"]
            .as_array()
            .expect("nodes")
            .is_empty()
    );

    // Rejecting whitespace must not strip or alter an otherwise valid payload.
    let response = router
        .oneshot(with_cookie(
            post_json("/api/v1/nodes/import", IMPORT_BODY),
            &cookie,
        ))
        .await
        .expect("valid import");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_to_json(response).await["new_nodes"], 2);
}
