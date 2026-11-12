//! A usable synthetic node is a precondition for ordinary subscription saves.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

pub async fn seed_generation_node(router: &axum::Router, cookie: &str) {
    let body = serde_json::json!({
        "source_type":"uri_list",
        "content":"trojan://TEST_PASSWORD@subscription-fixture.example:443#subscription-fixture"
    });
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/nodes/import")
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .body(Body::from(body.to_string()))
                .expect("fixture request"),
        )
        .await
        .expect("fixture import");
    assert_eq!(response.status(), StatusCode::OK);
}
