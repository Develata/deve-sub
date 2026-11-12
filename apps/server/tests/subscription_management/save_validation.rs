//! Save-time generation checks must precede credentials and preserve exact selection.

use super::*;
use serde_json::{Value, json};

async fn template_with_selector(router: &axum::Router, cookie: &str, selector: Value) -> String {
    let spec = json!({"apiVersion":"deve-sub.io/v1","kind":"SubscriptionTemplate",
        "metadata":{"name":"selection"},"spec":{"targetProfiles":["mihomo","json"],
        "nodeSelector":selector,"proxyGroups":[],"rules":[]}});
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &json!({"name":format!("selection-{}",ulid::Ulid::new()),
            "spec_yaml":spec.to_string()})
                .to_string(),
            ),
            cookie,
        ))
        .await
        .expect("template");
    assert_eq!(response.status(), StatusCode::CREATED);
    body_to_json(response).await["template"]["id"]
        .as_str()
        .expect("id")
        .to_owned()
}

async fn save(
    router: &axum::Router,
    cookie: &str,
    template: &str,
    selection: Value,
    profile: &str,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/subscriptions",
                &json!({"name":"selection","slug":format!("selection-{}",ulid::Ulid::new()),
            "template_id":template,"profile":profile,"node_selection":selection})
                .to_string(),
            ),
            cookie,
        ))
        .await
        .expect("save")
}

async fn row_counts(app: &TestApp) -> (i64, i64) {
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}?mode=ro",
        app._dir.path().join("test.db").display()
    ))
    .await
    .expect("read-only database");
    let subscriptions = sqlx::query_scalar("SELECT COUNT(*) FROM subscriptions")
        .fetch_one(&pool)
        .await
        .expect("subscriptions");
    let tokens = sqlx::query_scalar("SELECT COUNT(*) FROM subscription_tokens")
        .fetch_one(&pool)
        .await
        .expect("tokens");
    pool.close().await;
    (subscriptions, tokens)
}

async fn import_proxy(router: &axum::Router, cookie: &str, kind: &str, name: &str) -> String {
    let input = json!({"proxies":[{"name":name,"type":kind,"server":format!("{name}.example"),"port":1080}]});
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/nodes/import",
                &json!({"source_type":"mihomo_yaml","content":input.to_string()}).to_string(),
            ),
            cookie,
        ))
        .await
        .expect("import");
    assert_eq!(response.status(), StatusCode::OK);
    body_to_json(response).await["outcomes"][0]["data"]
        .as_str()
        .expect("id")
        .to_owned()
}

async fn names(router: &axum::Router, token: &str) -> Vec<String> {
    let response = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/json")))
        .await
        .expect("public delivery");
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_to_json(response).await;
    let mut names = body
        .as_array()
        .expect("canonical list")
        .iter()
        .map(|node| node["display_name"].as_str().expect("name").to_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[tokio::test]
async fn invalid_saves_never_persist_subscription_or_token() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template = template_with_selector(&router, &cookie, json!({"mode":"dynamic"})).await;
    let response = save(
        &router,
        &cookie,
        &template,
        json!({"mode":"inherit_template"}),
        "json",
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let error = body_to_json(response).await;
    assert_eq!(error["error"], "invalid_generation");
    assert!(error.get("token_plaintext").is_none());
    assert_eq!(row_counts(&app).await, (0, 0));

    import_proxy(&router, &cookie, "socks5", "usable").await;
    for (profile, selector) in [
        ("xray", json!({"mode":"inherit_template"})),
        ("json", json!({"mode":"fixed","nodeIds":[]})),
        (
            "json",
            json!({"mode":"dynamic","filters":[{"field":"protocol","value":"tuic"}]}),
        ),
    ] {
        let response = save(&router, &cookie, &template, selector, profile).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{profile}"
        );
        assert_eq!(body_to_json(response).await["error"], "invalid_generation");
        assert_eq!(row_counts(&app).await, (0, 0));
    }
}

#[tokio::test]
async fn inherited_fixed_and_explicit_selections_never_widen() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let socks = import_proxy(&router, &cookie, "socks5", "socks-only").await;
    let http = import_proxy(&router, &cookie, "http", "http-only").await;
    let template =
        template_with_selector(&router, &cookie, json!({"mode":"fixed","nodeIds":[socks]})).await;
    let mut tokens = Vec::new();
    for selection in [
        json!({"mode":"inherit_template"}),
        json!({"mode":"fixed","nodeIds":[http]}),
        json!({"mode":"dynamic","filters":[{"field":"protocol","value":"http"}]}),
    ] {
        let response = save(&router, &cookie, &template, selection.clone(), "json").await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let created = body_to_json(response).await;
        let token = created["token_plaintext"]
            .as_str()
            .expect("one-time token")
            .to_owned();
        let id = created["subscription"]["id"].as_str().expect("id");
        let response = router
            .clone()
            .oneshot(with_cookie(
                get(&format!("/api/v1/subscriptions/{id}")),
                &cookie,
            ))
            .await
            .expect("reopen");
        assert_eq!(
            body_to_json(response).await["subscription"]["node_selection"]["mode"],
            selection["mode"]
        );
        tokens.push(token);
    }
    assert_eq!(names(&router, &tokens[0]).await, ["socks-only"]);
    assert_eq!(names(&router, &tokens[1]).await, ["http-only"]);
    assert_eq!(names(&router, &tokens[2]).await, ["http-only"]);
    import_proxy(&router, &cookie, "http", "later-http").await;
    assert_eq!(names(&router, &tokens[0]).await, ["socks-only"]);
    assert_eq!(names(&router, &tokens[1]).await, ["http-only"]);
    assert_eq!(
        names(&router, &tokens[2]).await,
        ["http-only", "later-http"]
    );
    assert_eq!(row_counts(&app).await, (3, 3));
}

#[tokio::test]
async fn rejected_update_preserves_row_and_existing_credential() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let id = import_proxy(&router, &cookie, "http", "preserved").await;
    let template =
        template_with_selector(&router, &cookie, json!({"mode":"fixed","nodeIds":[id]})).await;
    let response = save(
        &router,
        &cookie,
        &template,
        json!({"mode":"inherit_template"}),
        "json",
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_to_json(response).await;
    let id = created["subscription"]["id"].as_str().expect("id");
    let path = format!("/api/v1/subscriptions/{id}");
    let update = json!({"name":"must-not-persist","slug":created["subscription"]["slug"],
        "profile":"json","node_selection":{"mode":"fixed","nodeIds":[]}});
    let response = router
        .clone()
        .oneshot(with_cookie(put_json(&path, &update.to_string()), &cookie))
        .await
        .expect("update");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let response = router
        .clone()
        .oneshot(with_cookie(get(&path), &cookie))
        .await
        .expect("get");
    let persisted = body_to_json(response).await;
    assert_eq!(
        persisted["subscription"]["name"],
        created["subscription"]["name"]
    );
    assert_eq!(
        persisted["subscription"]["node_selection"],
        created["subscription"]["node_selection"]
    );
    assert_eq!(row_counts(&app).await, (1, 1));
    assert_eq!(
        names(&router, created["token_plaintext"].as_str().expect("token")).await,
        ["preserved"]
    );
}

#[tokio::test]
async fn empty_pool_still_allows_revocation_but_reenable_requires_valid_generation() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let node = import_proxy(&router, &cookie, "http", "revocable").await;
    let template = template_with_selector(&router, &cookie, json!({"mode":"dynamic"})).await;
    let created = body_to_json(
        save(
            &router,
            &cookie,
            &template,
            json!({"mode":"inherit_template"}),
            "json",
        )
        .await,
    )
    .await;
    let path = format!(
        "/api/v1/subscriptions/{}",
        created["subscription"]["id"].as_str().expect("id")
    );
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/nodes/batch-enabled",
                &json!({"node_ids":[node],"enabled":false}).to_string(),
            ),
            &cookie,
        ))
        .await
        .expect("disable node");
    assert_eq!(response.status(), StatusCode::OK);
    let mut update = json!({
        "name":created["subscription"]["name"],"slug":created["subscription"]["slug"],
        "profile":"json","node_selection":{"mode":"inherit_template"},"enabled":false
    });
    let response = router
        .clone()
        .oneshot(with_cookie(put_json(&path, &update.to_string()), &cookie))
        .await
        .expect("revoke");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_to_json(response).await["subscription"]["enabled"],
        false
    );
    update["enabled"] = json!(true);
    let response = router
        .clone()
        .oneshot(with_cookie(put_json(&path, &update.to_string()), &cookie))
        .await
        .expect("reenable");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    update["enabled"] = json!(false);
    update["node_selection"] = json!({"mode":"fixed","nodeIds":[]});
    let response = router
        .clone()
        .oneshot(with_cookie(put_json(&path, &update.to_string()), &cookie))
        .await
        .expect("changed generation");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(row_counts(&app).await, (1, 1));
}
