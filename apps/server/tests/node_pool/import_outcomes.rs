use super::*;

/// NODE-001/003: failed lines occupy their original position alongside the
/// committed node IDs, including when the same payload is imported again.
#[tokio::test]
async fn mixed_import_outcomes_match_input_order_and_persisted_nodes() {
    for source_type in ["uri_list", "auto"] {
        let app = TestApp::new().await;
        let router = app.router();
        let cookie = setup_and_login(&router).await;
        let content = "\n# fixture comment\n\
            trojan://TEST_PASSWORD@example.com:443#NodeA\n\
            invalid-middle-TEST_INPUT\n\
            trojan://TEST_PASSWORD@example.net:8443#NodeB\n\n";
        let body = serde_json::json!({ "source_type": source_type, "content": content });
        let mut first_ids = Vec::new();
        for (status, new_nodes, duplicate_nodes) in [("inserted", 2, 0), ("duplicate", 0, 2)] {
            let response = router
                .clone()
                .oneshot(with_cookie(
                    post_json("/api/v1/nodes/import", &body.to_string()),
                    &cookie,
                ))
                .await
                .expect("mixed import");
            assert_eq!(response.status(), StatusCode::OK);
            let json = body_to_json(response).await;
            assert_eq!(json["new_nodes"], new_nodes);
            assert_eq!(json["duplicate_nodes"], duplicate_nodes);
            assert_eq!(json["failed"], 1);
            let outcomes = json["outcomes"].as_array().expect("outcomes");
            assert_eq!(outcomes.len(), 3, "one outcome for each non-comment entry");
            assert_eq!(outcomes[0]["status"], status);
            assert_eq!(
                outcomes[1],
                serde_json::json!({
                    "status": "failed", "data": "invalid-middle-TEST_INPUT"
                })
            );
            assert_eq!(outcomes[2]["status"], status);

            let ids = vec![outcomes[0]["data"].clone(), outcomes[2]["data"].clone()];
            if first_ids.is_empty() {
                first_ids = ids;
            } else {
                assert_eq!(ids, first_ids, "duplicates refer to the committed nodes");
            }
        }

        let response = router
            .oneshot(with_cookie(get("/api/v1/nodes"), &cookie))
            .await
            .expect("list");
        let listed = body_to_json(response).await;
        let nodes = listed["nodes"].as_array().expect("nodes");
        assert_eq!(nodes.len(), 2);
        for (id, name) in first_ids.iter().zip(["NodeA", "NodeB"]) {
            let node = nodes
                .iter()
                .find(|node| node["id"] == *id)
                .expect("saved node");
            assert_eq!(node["display_name"], name);
        }
    }
}

/// NODE-001: consecutive failures at both ends must not shift successful IDs.
#[tokio::test]
async fn failed_import_outcomes_preserve_leading_trailing_and_all_failed_entries() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    for (content, expected) in [
        (
            "invalid-first\ninvalid-second\ntrojan://TEST_PASSWORD@example.com:443#NodeA\ninvalid-last",
            vec!["failed", "failed", "inserted", "failed"],
        ),
        (
            "invalid-first\n# ignored\n\ninvalid-last",
            vec!["failed", "failed"],
        ),
    ] {
        let body = serde_json::json!({ "source_type": "uri_list", "content": content });
        let response = router
            .clone()
            .oneshot(with_cookie(
                post_json("/api/v1/nodes/import", &body.to_string()),
                &cookie,
            ))
            .await
            .expect("import");
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_to_json(response).await;
        let outcomes = json["outcomes"].as_array().expect("outcomes");
        let statuses: Vec<_> = outcomes
            .iter()
            .map(|outcome| outcome["status"].as_str().expect("status"))
            .collect();
        assert_eq!(statuses, expected);
        let failed: Vec<_> = outcomes
            .iter()
            .filter(|outcome| outcome["status"] == "failed")
            .map(|outcome| outcome["data"].as_str().expect("raw input"))
            .collect();
        let invalid_lines: Vec<_> = content
            .lines()
            .filter(|line| line.starts_with("invalid-"))
            .collect();
        assert_eq!(failed, invalid_lines);
        assert_eq!(json["failed"], failed.len());
    }
}
