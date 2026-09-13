use super::*;
use axum::extract::ConnectInfo;
use std::net::SocketAddr;

#[tokio::test]
async fn direct_peer_ip_limits_rotating_usernames_and_ignores_spoofed_headers() {
    let app = TestApp::with_max_attempts(2).await;
    let router = app.router();
    for (index, expected) in [
        StatusCode::UNAUTHORIZED,
        StatusCode::UNAUTHORIZED,
        StatusCode::TOO_MANY_REQUESTS,
    ]
    .into_iter()
    .enumerate()
    {
        let mut request = post_json_with_xff(
            "/api/v1/auth/login",
            &format!(r#"{{"username":"fixture-{index}","password":"wrong-password"}}"#),
            &format!("192.0.2.{}", index + 1),
        );
        request.extensions_mut().insert(ConnectInfo(
            "198.51.100.10:12345".parse::<SocketAddr>().expect("peer"),
        ));
        let response = router.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), expected);
    }
}

#[tokio::test]
async fn sensitive_responses_are_not_cached_or_framed() {
    let app = TestApp::new().await;
    let router = app.router();
    for path in [
        "/api/v1/auth/status",
        "/api/v1/auth/me",
        "/api/v1/subscriptions",
        "/",
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(
            response
                .headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    }
}

#[tokio::test]
async fn short_code_probes_have_an_independent_ip_budget() {
    let mut app = TestApp::with_max_attempts(2).await;
    app.state.short_code_rate_limiter = Arc::new(deve_sub_inmemory::InMemoryLoginRateLimiter::new(
        2,
        std::time::Duration::from_secs(60),
    ));
    let router = app.router();
    for expected in [
        StatusCode::NOT_FOUND,
        StatusCode::NOT_FOUND,
        StatusCode::TOO_MANY_REQUESTS,
    ] {
        let mut request = Request::builder()
            .uri("/s/fixture-missing/mihomo")
            .body(Body::empty())
            .expect("request");
        request.extensions_mut().insert(ConnectInfo(
            "198.51.100.10:12345".parse::<SocketAddr>().expect("peer"),
        ));
        assert_eq!(
            router
                .clone()
                .oneshot(request)
                .await
                .expect("response")
                .status(),
            expected
        );
    }
    let mut request = post_json(
        "/api/v1/auth/login",
        r#"{"username":"fixture-user","password":"wrong-password"}"#,
    );
    request.extensions_mut().insert(ConnectInfo(
        "198.51.100.10:12345".parse::<SocketAddr>().expect("peer"),
    ));
    assert_eq!(
        router.oneshot(request).await.expect("response").status(),
        StatusCode::UNAUTHORIZED
    );
}
