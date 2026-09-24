//! OUT-016: invalid public integers cannot poison persisted subscription rows.

use super::*;

#[tokio::test]
async fn out016_integer_bounds_reject_overflow_without_mutation() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template = create_template(&router, &cookie).await;
    let mut body: serde_json::Value =
        serde_json::from_str(&create_sub_body("bounded", "bounded", &template)).expect("body");
    let maximum = i64::MAX as u64;
    for invalid in [0, maximum + 1, u64::MAX] {
        body["traffic_limit"] = invalid.into();
        let response = router
            .clone()
            .oneshot(with_cookie(
                post_json("/api/v1/subscriptions", &body.to_string()),
                &cookie,
            ))
            .await
            .expect("request");
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "limit {invalid}"
        );
    }

    body["traffic_limit"] = maximum.into();
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/subscriptions", &body.to_string()),
            &cookie,
        ))
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_to_json(response).await;
    let id = created["subscription"]["id"].as_str().expect("id");
    let path = format!("/api/v1/subscriptions/{id}");
    assert_eq!(
        created["subscription"]["traffic_limit"].as_u64(),
        Some(maximum)
    );

    body.as_object_mut().expect("object").remove("template_id");
    for field in ["traffic_limit", "template_version_pin"] {
        for invalid in [maximum + 1, u64::MAX] {
            let mut update = body.clone();
            update[field] = invalid.into();
            let response = router
                .clone()
                .oneshot(with_cookie(put_json(&path, &update.to_string()), &cookie))
                .await
                .expect("request");
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{field} {invalid}"
            );
        }
    }

    let response = router
        .clone()
        .oneshot(with_cookie(get(&path), &cookie))
        .await
        .expect("get");
    assert_eq!(response.status(), StatusCode::OK);
    let persisted = body_to_json(response).await;
    assert_eq!(
        persisted["subscription"]["traffic_limit"].as_u64(),
        Some(maximum)
    );
    assert!(persisted["subscription"]["template_version_pin"].is_null());
    let response = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/subscriptions"), &cookie))
        .await
        .expect("list");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_to_json(response).await["subscriptions"]
            .as_array()
            .expect("list")
            .len(),
        1
    );

    // Adapter callers must not bypass checked encoding. All three mutation
    // entry points must reject before an INSERT/UPDATE can wrap a u64.
    let id = deve_sub_kernel::SubscriptionId::parse(id).expect("subscription id");
    let original = app
        .state
        .subscription_repo
        .find_by_id(id)
        .await
        .expect("find")
        .expect("row");
    let token = app
        .state
        .subscription_token_repo
        .find_active_for_subscription(id)
        .await
        .expect("find token")
        .expect("token");
    for field in ["traffic_limit", "template_version_pin"] {
        let mut invalid = original.clone();
        if field == "traffic_limit" {
            invalid.traffic_limit = Some(maximum + 1);
        } else {
            invalid.template_version_pin = Some(maximum + 1);
        }
        let results = [
            app.state.subscription_repo.create(&invalid).await,
            app.state
                .subscription_repo
                .create_with_token(&invalid, &token)
                .await,
            app.state.subscription_repo.update(&invalid).await,
        ];
        for result in results {
            let error = result.expect_err("reject overflow").to_string();
            assert!(
                error.contains(field) && error.contains("storage range"),
                "{error}"
            );
        }
    }
    let row = app
        .state
        .subscription_repo
        .find_by_id(id)
        .await
        .expect("find")
        .expect("row");
    assert_eq!(row.traffic_limit, original.traffic_limit);
    assert_eq!(row.template_version_pin, original.template_version_pin);
}
