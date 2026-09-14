//! GEN-015: unsupported Mihomo groups are actionable errors, never published output.

use super::*;

#[tokio::test]
async fn gen015_unsupported_group_returns_422_and_preserves_active() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let nodes = import_nodes(
        &router,
        &cookie,
        "trojan://fixture@node.example.com:443#fixture",
    )
    .await;
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &create_body("group-safety", "test", VALID_SPEC_YAML),
            ),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = body_to_json(response).await["template"]["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let generate_path = format!("/api/v1/templates/{id}/generate?profile=mihomo&mode=lenient");
    let response = router
        .clone()
        .oneshot(with_cookie(post_json(&generate_path, ""), &cookie))
        .await
        .expect("good generation");
    assert_eq!(response.status(), StatusCode::OK);
    let good = body_to_json(response).await["content"].clone();
    for kind in ["direct", "reject"] {
        let groups = serde_json::json!([{"name":"unsupported", "type":kind, "members":[{"kind":"node", "id":nodes[0]}]}]);
        let spec = VALID_SPEC_YAML.replace("proxyGroups: []", &format!("proxyGroups: {groups}"));
        let response = router
            .clone()
            .oneshot(with_cookie(
                put_json(
                    &format!("/api/v1/templates/{id}"),
                    &update_body("group-safety", "test", &spec),
                ),
                &cookie,
            ))
            .await
            .expect("update");
        assert_eq!(response.status(), StatusCode::OK);
        for surface in ["generate", "preview"] {
            for mode in ["strict", "lenient"] {
                let response = router
                    .clone()
                    .oneshot(with_cookie(
                        post_json(
                            &format!("/api/v1/templates/{id}/{surface}?profile=mihomo&mode={mode}"),
                            "",
                        ),
                        &cookie,
                    ))
                    .await
                    .expect("generation error");
                assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
                let error = body_to_json(response).await;
                assert_eq!(error["error"], "incompatible_groups");
                assert!(error["message"].as_str().expect("message").contains(kind));
            }
        }
        let response = router
            .clone()
            .oneshot(with_cookie(
                get(&format!(
                    "/api/v1/templates/{id}/generations/active?profile=mihomo"
                )),
                &cookie,
            ))
            .await
            .expect("active");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_to_json(response).await["content"], good);
    }
}
