//! Current credentials belong only to the authorized owner page.

use super::*;
use deve_sub_kernel::UserId;

#[tokio::test]
async fn subscription_list_projects_current_codes_and_preserves_owner_pagination() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    import_nodes(&router, &cookie, "trojan://TEST@list.example:443#list").await;
    let template = create_template(&router, &cookie).await;
    let first = create_sub(&router, &cookie, &template, "first").await;
    let second = create_sub(&router, &cookie, &template, "second").await;
    let first_id = first["subscription"]["id"].as_str().expect("first id");
    let second_id = second["subscription"]["id"].as_str().expect("second id");
    let mut codes = std::collections::HashMap::new();
    for id in [first_id, second_id, first_id] {
        let response = router
            .clone()
            .oneshot(with_cookie(
                post_json(
                    &format!("/api/v1/subscriptions/{id}/regenerate-short-code"),
                    "",
                ),
                &cookie,
            ))
            .await
            .expect("regenerate");
        assert_eq!(response.status(), StatusCode::OK);
        codes.insert(id, body_to_json(response).await["code"].clone());
    }
    let mut path = "/api/v1/subscriptions?limit=1".to_owned();
    let mut seen = Vec::new();
    for _ in 0..2 {
        let response = router
            .clone()
            .oneshot(with_cookie(get(&path), &cookie))
            .await
            .expect("page");
        assert_eq!(response.status(), StatusCode::OK);
        let page = body_to_json(response).await;
        let rows = page["subscriptions"].as_array().expect("rows");
        assert_eq!(rows.len(), 1);
        let id = rows[0]["id"].as_str().expect("id");
        assert_eq!(rows[0]["short_code"], codes[id]);
        assert!(rows[0].get("token_plaintext").is_none());
        seen.push(id.to_owned());
        path = format!(
            "/api/v1/subscriptions?limit=1&cursor={}",
            page["next_cursor"].as_str().expect("cursor")
        );
    }
    seen.sort();
    let mut expected = vec![first_id.to_owned(), second_id.to_owned()];
    expected.sort();
    assert_eq!(seen, expected);
    let other_owner = deve_sub_application::subscription::list_subscriptions_with_short_codes(
        app.state.subscription_repo.as_ref(),
        app.state.short_code_repo.as_ref(),
        UserId::new(),
        None,
        Some(100),
    )
    .await
    .expect("other owner");
    assert!(other_owner.is_empty());
}
