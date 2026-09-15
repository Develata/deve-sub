//! OUT-008/014: withdrawal applies to every public URL and concurrent publisher.
use super::*;
use deve_sub_domain::{
    ItemParseStatus, ReconcileEntry, ReconcileInput, Source, SourceSnapshot, SourceType,
};
use deve_sub_kernel::{NodeId, SourceId, SourceSnapshotId, Timestamp};
use serde_json::json;
use std::time::Duration;

#[path = "source_withdrawal/gated_cache.rs"]
mod gated_cache;

async fn remote(app: &TestApp) -> (SourceId, NodeId) {
    let source = Source::new(
        "remote",
        SourceType::UriList,
        "https://source.example.com/sub".into(),
    );
    app.state.source_repo.create(&source).await.expect("source");
    let mut node = deve_sub_application::source::parse_for_import(
        SourceType::UriList,
        None,
        b"trojan://fixture@remote.example.com:443#remote",
    )
    .expect("parse")
    .nodes
    .pop()
    .expect("node");
    node.source.source_label.clear();
    let id = node.id;
    app.state
        .pool_repo
        .reconcile(ReconcileInput {
            source_id: source.id,
            snapshot: &SourceSnapshot {
                id: SourceSnapshotId::new(),
                source_id: source.id,
                version: 1,
                fetched_at: Timestamp::now(),
                etag: None,
                node_count: 1,
                is_active: true,
            },
            entries: &[ReconcileEntry {
                raw_uri: "fixture".into(),
                initial_status: ItemParseStatus::Parsed,
                node: Some(node),
            }],
        })
        .await
        .expect("refresh");
    (source.id, id)
}

async fn delete(router: &axum::Router, cookie: &str, id: SourceId) {
    let response = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/sources/{id}"))
                .body(Body::empty())
                .expect("request"),
            cookie,
        ))
        .await
        .expect("delete");
    assert_eq!(response.status(), StatusCode::OK);
}

async fn public_paths(router: &axum::Router, cookie: &str, sub: &serde_json::Value) -> Vec<String> {
    let id = sub["subscription"]["id"].as_str().expect("id");
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                &format!("/api/v1/subscriptions/{id}/regenerate-short-code"),
                "",
            ),
            cookie,
        ))
        .await
        .expect("short");
    assert_eq!(response.status(), StatusCode::OK);
    let code = body_to_json(response).await["code"]
        .as_str()
        .expect("code")
        .to_owned();
    let expiry = (time::OffsetDateTime::now_utc() + time::Duration::hours(1))
        .format(&time::format_description::well_known::Rfc3339)
        .expect("expiry");
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                &format!("/api/v1/subscriptions/{id}/temp-links"),
                &json!({"expires_at":expiry}).to_string(),
            ),
            cookie,
        ))
        .await
        .expect("temp");
    assert_eq!(response.status(), StatusCode::CREATED);
    let temp = body_to_json(response).await["token_plaintext"]
        .as_str()
        .expect("token")
        .to_owned();
    vec![
        format!(
            "/sub/{}/mihomo",
            sub["token_plaintext"].as_str().expect("token")
        ),
        format!("/s/{code}/mihomo"),
        format!("/sub/{temp}/mihomo"),
    ]
}

#[tokio::test]
async fn out014_source_delete_updates_all_urls_and_never_confirms_withdrawn_etag() {
    for mode in ["all", "fixed-both", "fixed-withdrawn", "source", "pinned"] {
        let app = TestApp::new().await;
        let router = app.router();
        let cookie = setup_and_login(&router).await;
        let (source, withdrawn) = remote(&app).await;
        let retained = import_nodes(
            &router,
            &cookie,
            "trojan://fixture@manual.example.com:443#manual",
        )
        .await;
        let template = create_template(&router, &cookie).await;
        let selection = match mode {
            "all" => json!({"mode":"dynamic"}),
            "fixed-both" => json!({"mode":"fixed", "nodeIds":[withdrawn.to_string(), retained[0]]}),
            "source" => json!({"mode":"dynamic", "filters":[{"field":"source", "value":"remote"}]}),
            _ => json!({"mode":"fixed", "nodeIds":[withdrawn.to_string()]}),
        };
        let response = router
            .clone()
            .oneshot(with_cookie(
                post_json(
                    "/api/v1/subscriptions",
                    &json!({"name":mode,"slug":mode,"template_id":template,"profile":"mihomo",
                "node_selection":selection })
                    .to_string(),
                ),
                &cookie,
            ))
            .await
            .expect("subscription");
        let status = response.status();
        let sub = body_to_json(response).await;
        assert_eq!(status, StatusCode::CREATED, "{mode}");
        if mode == "pinned" {
            let id = sub["subscription"]["id"].as_str().expect("id");
            let response = router.clone().oneshot(with_cookie(put_json(
                &format!("/api/v1/subscriptions/{id}"),
                &json!({"name":mode,"slug":mode,"profile":"mihomo","node_selection":selection,
                    "template_version_pin":1}).to_string()), &cookie)).await.expect("pin");
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                body_to_json(response).await["subscription"]["template_version_pin"],
                1
            );
        }
        let paths = public_paths(&router, &cookie, &sub).await;
        let before = router
            .clone()
            .oneshot(get(&paths[0]))
            .await
            .expect("before");
        assert_eq!(before.status(), StatusCode::OK);
        let etag = before.headers()["etag"].to_str().expect("etag").to_owned();
        assert!(body_to_string(before).await.contains("remote.example.com"));
        delete(&router, &cookie, source).await;
        let mut requests = tokio::task::JoinSet::new();
        for path in paths {
            let router = router.clone();
            let etag = etag.clone();
            requests.spawn(async move {
                for conditional in [false, true] {
                    let mut request = get(&path);
                    if conditional {
                        request = request.with_header("if-none-match", etag.clone());
                    }
                    let response = router.clone().oneshot(request).await.expect("after");
                    if matches!(mode, "all" | "fixed-both") {
                        assert_eq!(response.status(), StatusCode::OK, "{mode}");
                        let current = response.headers()["etag"]
                            .to_str()
                            .expect("etag")
                            .to_owned();
                        assert_ne!(current, etag);
                        let body = body_to_string(response).await;
                        assert!(!body.contains("remote.example.com"));
                        assert!(body.contains("manual.example.com"));
                        let unchanged = router
                            .clone()
                            .oneshot(get(&path).with_header("if-none-match", current))
                            .await
                            .expect("unchanged");
                        assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
                    } else {
                        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{mode}");
                        assert!(!response.headers().contains_key("etag"));
                        let body = body_to_string(response).await;
                        assert!(body.contains("No available nodes"));
                        assert!(!body.contains("remote.example.com"));
                    }
                }
            });
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(result) = requests.join_next().await {
                result.expect("public surface");
            }
        })
        .await
        .expect("concurrent delivery deadline");
    }
}

#[tokio::test]
async fn out014_late_generation_cannot_replace_post_delete_output() {
    let mut app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let (source, _) = remote(&app).await;
    import_nodes(
        &router,
        &cookie,
        "trojan://fixture@manual.example.com:443#manual",
    )
    .await;
    let template = create_template(&router, &cookie).await;
    let sub = create_sub(&router, &cookie, &template, "late").await;
    let gate = Arc::new(gated_cache::GatedCache::new(app.state.cache_repo.clone()));
    app.state.cache_repo = gate.clone();
    let router = app.router();
    let path = format!("/api/v1/templates/{template}/generate?profile=mihomo&mode=lenient");
    let old = tokio::spawn(
        router
            .clone()
            .oneshot(with_cookie(post_json(&path, ""), &cookie)),
    );
    tokio::time::timeout(Duration::from_secs(10), gate.entered.notified())
        .await
        .expect("old snapshot reached store");
    delete(&router, &cookie, source).await;
    let fresh = router
        .clone()
        .oneshot(with_cookie(post_json(&path, ""), &cookie))
        .await
        .expect("fresh");
    assert_eq!(fresh.status(), StatusCode::OK);
    let fresh = body_to_json(fresh).await["content"]
        .as_str()
        .expect("content")
        .to_owned();
    assert!(!fresh.contains("remote.example.com"));
    assert!(fresh.contains("manual.example.com"));
    gate.release.notify_one();
    let old = tokio::time::timeout(Duration::from_secs(10), old)
        .await
        .expect("deadline")
        .expect("task")
        .expect("response");
    assert_eq!(old.status(), StatusCode::CONFLICT);
    assert_eq!(body_to_json(old).await["error"], "generation_invalidated");
    let paths = public_paths(&router, &cookie, &sub).await;
    for path in paths {
        let response = router.clone().oneshot(get(&path)).await.expect("public");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_to_string(response).await, fresh);
    }
}
