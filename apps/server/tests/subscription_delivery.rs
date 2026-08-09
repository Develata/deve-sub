#![allow(clippy::expect_used)]

//! Integration tests for the public subscription delivery surface (M6 Slice 2).
//!
//! Covers `/sub/{token}/{profile}` and `/sub/{token}` delivery: ETag/304
//! conditional GET (OUT-008), bad token 404 with no existence leak (OUT-009),
//! User-Agent auto-detect, cache hit/miss, disabled subscription 404, wrong
//! explicit profile 404, and `subscription-userinfo` header. See
//! `docs/plan/milestones/M6-subscription-distribution.md` Slice 2.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use deve_sub_application::{DbHealthPort, GeoIpPort, LoginRateLimiter, SubscriptionFetcher};
use deve_sub_domain::{
    GenerationCacheRepository, NodeOverrideRepository, NodePoolRepository, PoolMetaRepository,
    RecoveryCodeRepository, SessionRepository, SourceRepository, SourceSnapshotRepository,
    SubscriptionRepository, SubscriptionTokenRepository, TemplateRepository,
    TemplateVersionRepository, TotpSecretRepository, UserRepository,
};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{
    SqliteGenerationCacheRepository, SqliteHealthCheck, SqliteNodeOverrideRepository,
    SqliteNodePoolRepository, SqlitePoolMetaRepository, SqliteRecoveryCodeRepository,
    SqliteSessionRepository, SqliteSourceRepository, SqliteSourceSnapshotRepository,
    SqliteSubscriptionRepository, SqliteSubscriptionTokenRepository, SqliteTemplateRepository,
    SqliteTemplateVersionRepository, SqliteTotpSecretRepository, SqliteUserRepository,
};

struct TestApp {
    state: deve_sub_server::AppState,
    _dir: tempfile::TempDir,
}

impl TestApp {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("test.db");
        let key_path = dir.path().join("master.key");

        let pool =
            sqlx::sqlite::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
                .await
                .expect("pool");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("migrations");

        let master_key = Arc::new(MasterKey::load_or_generate(&key_path).expect("master key"));
        let config = deve_sub_application::AppConfig::default();

        let rate_limiter: Arc<dyn LoginRateLimiter> =
            Arc::new(deve_sub_inmemory::InMemoryLoginRateLimiter::new(
                config.security.max_login_attempts,
                std::time::Duration::from_secs(config.security.lockout_duration_secs),
            ));

        let db_health: Arc<dyn DbHealthPort> = Arc::new(SqliteHealthCheck::new(pool.clone()));

        Self {
            state: deve_sub_server::AppState {
                config,
                master_key,
                user_repo: Arc::new(SqliteUserRepository::new(pool.clone()))
                    as Arc<dyn UserRepository>,
                session_repo: Arc::new(SqliteSessionRepository::new(pool.clone()))
                    as Arc<dyn SessionRepository>,
                totp_secret_repo: Arc::new(SqliteTotpSecretRepository::new(pool.clone()))
                    as Arc<dyn TotpSecretRepository>,
                recovery_code_repo: Arc::new(SqliteRecoveryCodeRepository::new(pool.clone()))
                    as Arc<dyn RecoveryCodeRepository>,
                source_repo: Arc::new(SqliteSourceRepository::new(pool.clone()))
                    as Arc<dyn SourceRepository>,
                snapshot_repo: Arc::new(SqliteSourceSnapshotRepository::new(pool.clone()))
                    as Arc<dyn SourceSnapshotRepository>,
                pool_repo: Arc::new(SqliteNodePoolRepository::new(pool.clone()))
                    as Arc<dyn NodePoolRepository>,
                pool_meta_repo: Arc::new(SqlitePoolMetaRepository::new(pool.clone()))
                    as Arc<dyn PoolMetaRepository>,
                override_repo: Arc::new(SqliteNodeOverrideRepository::new(pool.clone()))
                    as Arc<dyn NodeOverrideRepository>,
                template_repo: Arc::new(SqliteTemplateRepository::new(pool.clone()))
                    as Arc<dyn TemplateRepository>,
                version_repo: Arc::new(SqliteTemplateVersionRepository::new(pool.clone()))
                    as Arc<dyn TemplateVersionRepository>,
                cache_repo: Arc::new(SqliteGenerationCacheRepository::new(pool.clone()))
                    as Arc<dyn GenerationCacheRepository>,
                subscription_repo: Arc::new(SqliteSubscriptionRepository::new(pool.clone()))
                    as Arc<dyn SubscriptionRepository>,
                subscription_token_repo: Arc::new(SqliteSubscriptionTokenRepository::new(
                    pool.clone(),
                )) as Arc<dyn SubscriptionTokenRepository>,
                geoip: Arc::new(deve_sub_inmemory::InMemoryGeoIp::new()) as Arc<dyn GeoIpPort>,
                fetcher: Arc::new(deve_sub_adapters::HttpFetcher::new())
                    as Arc<dyn SubscriptionFetcher>,
                rate_limiter,
                db_health,
            },
            _dir: dir,
        }
    }

    fn router(&self) -> axum::Router {
        deve_sub_server::build_router(self.state.clone())
    }
}

fn json_body(json: &str) -> Body {
    Body::from(json.to_owned())
}

fn post_json(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(json_body(body))
        .expect("request")
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("request")
}

fn put_json(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(uri)
        .header("content-type", "application/json")
        .body(json_body(body))
        .expect("request")
}

trait RequestExt {
    fn with_header(self, key: &str, value: String) -> Self;
}

impl RequestExt for Request<Body> {
    fn with_header(mut self, key: &str, value: String) -> Self {
        use std::str::FromStr;
        let name = axum::http::HeaderName::from_str(key).expect("header name");
        self.headers_mut()
            .insert(name, value.parse().expect("header"));
        self
    }
}

fn with_cookie(req: Request<Body>, cookie: &str) -> Request<Body> {
    req.with_header("cookie", cookie.to_owned())
}

fn extract_cookie(response: &axum::response::Response) -> Option<String> {
    let cookies = response.headers().get("set-cookie")?.to_str().ok()?;
    let part = cookies
        .split(';')
        .find(|s| s.trim().starts_with("deve_sub_session="))?;
    Some(part.trim().to_owned())
}

async fn setup_and_login(router: &axum::Router) -> String {
    let _ = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/setup",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("setup");

    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("login");

    assert_eq!(response.status(), StatusCode::OK);
    extract_cookie(&response).expect("cookie")
}

async fn body_to_json(response: axum::response::Response) -> serde_json::Value {
    let body = axum::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("body");
    serde_json::from_slice(&body).expect("json")
}

async fn body_to_string(response: axum::response::Response) -> String {
    let body = axum::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("body");
    String::from_utf8(body.to_vec()).expect("utf8")
}

const VALID_SPEC_YAML: &str = concat!(
    "apiVersion: deve-sub.io/v1\n",
    "kind: SubscriptionTemplate\n",
    "\n",
    "metadata:\n",
    "  name: default-mihomo\n",
    "  description: Default Mihomo template\n",
    "  version: 1\n",
    "\n",
    "spec:\n",
    "  targetProfiles:\n",
    "    - mihomo\n",
    "  variables: {}\n",
    "  nodeSelector:\n",
    "    mode: dynamic\n",
    "  proxyGroups: []\n",
    "  rules: []\n",
    "  dns: {}\n",
    "  tun: {}\n",
    "  output: {}",
);

async fn create_template(router: &axum::Router, cookie: &str) -> String {
    let body = serde_json::json!({
        "name": "test-template",
        "description": "test",
        "spec_yaml": VALID_SPEC_YAML,
    })
    .to_string();
    let res = router
        .clone()
        .oneshot(with_cookie(post_json("/api/v1/templates", &body), cookie))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::CREATED);
    let v = body_to_json(res).await;
    v["template"]["id"]
        .as_str()
        .expect("template id")
        .to_owned()
}

async fn import_nodes(router: &axum::Router, cookie: &str, content: &str) -> Vec<String> {
    let body = serde_json::json!({
        "content": content,
        "source_type": "uri_list",
    })
    .to_string();
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/nodes/import", &body),
            cookie,
        ))
        .await
        .expect("import");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    json["outcomes"]
        .as_array()
        .expect("outcomes array")
        .iter()
        .filter_map(|o| {
            let status = o["status"].as_str().expect("status");
            if status == "inserted" || status == "duplicate" {
                Some(o["data"].as_str().expect("id").to_owned())
            } else {
                None
            }
        })
        .collect()
}

async fn create_sub(
    router: &axum::Router,
    cookie: &str,
    template_id: &str,
    slug: &str,
) -> serde_json::Value {
    let body = serde_json::json!({
        "name": slug,
        "slug": slug,
        "template_id": template_id,
        "profile": "mihomo",
        "node_selection": {"mode": "dynamic"},
    })
    .to_string();
    let res = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/subscriptions", &body),
            cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::CREATED);
    body_to_json(res).await
}

/// DEL-001 (OUT-008): `GET /sub/{token}/{profile}` returns 200 with correct
/// headers (ETag, Content-Type, Content-Disposition, subscription-userinfo,
/// Cache-Control) and non-empty content.
#[tokio::test]
async fn del001_delivery_returns_content_and_headers() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let _ = import_nodes(&router, &cookie, "trojan://pw@host-a.example.com:443#NodeA").await;
    let template_id = create_template(&router, &cookie).await;
    let v = create_sub(&router, &cookie, &template_id, "my-sub").await;
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    let res = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/mihomo")))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::OK);

    let etag = res
        .headers()
        .get("etag")
        .expect("etag header")
        .to_str()
        .expect("etag str")
        .to_owned();
    assert!(etag.starts_with('"') && etag.ends_with('"'), "strong ETag");
    assert_eq!(etag.len(), 66, "quoted 64-char hex");

    let content_type = res
        .headers()
        .get("content-type")
        .expect("content-type")
        .to_str()
        .expect("str");
    assert!(
        content_type.contains("yaml"),
        "mihomo content-type should be yaml, got: {content_type}"
    );

    let content_disposition = res
        .headers()
        .get("content-disposition")
        .expect("content-disposition")
        .to_str()
        .expect("str");
    assert!(
        content_disposition.contains("my-sub.yaml"),
        "content-disposition should include slug+ext, got: {content_disposition}"
    );

    let cache_control = res
        .headers()
        .get("cache-control")
        .expect("cache-control")
        .to_str()
        .expect("str");
    assert_eq!(
        cache_control, "private, no-cache",
        "cache-control should be private, no-cache"
    );

    let userinfo = res
        .headers()
        .get("subscription-userinfo")
        .expect("subscription-userinfo")
        .to_str()
        .expect("str");
    assert!(
        userinfo.contains("upload=0") && userinfo.contains("download=0"),
        "subscription-userinfo should report zero traffic in Slice 2, got: {userinfo}"
    );
    assert!(
        userinfo.contains("expire=0"),
        "no-expiry subscription should report expire=0, got: {userinfo}"
    );

    let content = body_to_string(res).await;
    assert!(!content.is_empty(), "content must not be empty");
    // Mihomo output is YAML; a valid trojan proxy should appear.
    assert!(
        content.contains("trojan") || content.contains("proxies"),
        "mihomo YAML should contain trojan or proxies, got first 200 chars: {}",
        &content[..content.len().min(200)]
    );
}

/// DEL-002 (OUT-008): A conditional GET with a matching `If-None-Match`
/// returns 304 Not Modified.
#[tokio::test]
async fn del002_conditional_get_returns_304() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let _ = import_nodes(&router, &cookie, "trojan://pw@host-a.example.com:443#NodeA").await;
    let template_id = create_template(&router, &cookie).await;
    let v = create_sub(&router, &cookie, &template_id, "sub-304").await;
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    // First request: get the ETag.
    let res1 = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/mihomo")))
        .await
        .expect("first");
    assert_eq!(res1.status(), StatusCode::OK);
    let etag = res1
        .headers()
        .get("etag")
        .expect("etag")
        .to_str()
        .expect("str")
        .to_owned();

    // Second request with If-None-Match: should 304.
    let res2 = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/mihomo")).with_header("if-none-match", etag.clone()))
        .await
        .expect("second");
    assert_eq!(res2.status(), StatusCode::NOT_MODIFIED);

    // 304 should still carry the ETag.
    let etag2 = res2
        .headers()
        .get("etag")
        .expect("etag on 304")
        .to_str()
        .expect("str");
    assert_eq!(etag2, etag, "304 ETag must match the 200 ETag");

    // 304 body should be empty.
    let body = body_to_string(res2).await;
    assert!(body.is_empty(), "304 body must be empty, got: {body}");
}

/// DEL-003 (OUT-008): A conditional GET with a non-matching `If-None-Match`
/// returns 200 with full content.
#[tokio::test]
async fn del003_non_matching_etag_returns_200() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let _ = import_nodes(&router, &cookie, "trojan://pw@host-a.example.com:443#NodeA").await;
    let template_id = create_template(&router, &cookie).await;
    let v = create_sub(&router, &cookie, &template_id, "sub-200").await;
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    let res = router
        .clone()
        .oneshot(
            get(&format!("/sub/{token}/mihomo"))
                .with_header("if-none-match", "\"stale-etag\"".to_owned()),
        )
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::OK);
    let content = body_to_string(res).await;
    assert!(
        !content.is_empty(),
        "non-matching ETag should return content"
    );
}

/// DEL-004 (OUT-008): `If-None-Match: *` always matches → 304.
#[tokio::test]
async fn del004_if_none_match_star_returns_304() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let _ = import_nodes(&router, &cookie, "trojan://pw@host-a.example.com:443#NodeA").await;
    let template_id = create_template(&router, &cookie).await;
    let v = create_sub(&router, &cookie, &template_id, "sub-star").await;
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    // First request primes the cache so generation succeeds.
    let _ = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/mihomo")))
        .await
        .expect("prime");

    let res = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/mihomo")).with_header("if-none-match", "*".to_owned()))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
}

/// DEL-005 (OUT-009): A bad token returns 404 with a generic body (no
/// existence leak). The body must not reveal whether the token, subscription,
/// or owner exists.
#[tokio::test]
async fn del005_bad_token_returns_404_no_leak() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let _ = import_nodes(&router, &cookie, "trojan://pw@host-a.example.com:443#NodeA").await;
    let template_id = create_template(&router, &cookie).await;
    let _ = create_sub(&router, &cookie, &template_id, "real-sub").await;

    let bad_token = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let res = router
        .clone()
        .oneshot(get(&format!("/sub/{bad_token}/mihomo")))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body = body_to_string(res).await;
    assert!(
        !body.contains("subscription") && !body.contains("token") && !body.contains("owner"),
        "404 body must not leak existence: {body}"
    );
}

/// DEL-006 (OUT-009): A disabled subscription returns 404 (no existence leak).
#[tokio::test]
async fn del006_disabled_subscription_returns_404() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let _ = import_nodes(&router, &cookie, "trojan://pw@host-a.example.com:443#NodeA").await;
    let template_id = create_template(&router, &cookie).await;
    let v = create_sub(&router, &cookie, &template_id, "disabled-sub").await;
    let sub_id = v["subscription"]["id"].as_str().expect("id").to_owned();
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    // Disable the subscription via PUT (enabled: false).
    let update_body = serde_json::json!({
        "name": "disabled-sub",
        "slug": "disabled-sub",
        "profile": "mihomo",
        "node_selection": {"mode": "dynamic"},
        "enabled": false,
    })
    .to_string();
    let res = router
        .clone()
        .oneshot(with_cookie(
            put_json(&format!("/api/v1/subscriptions/{sub_id}"), &update_body),
            &cookie,
        ))
        .await
        .expect("update");
    assert_eq!(res.status(), StatusCode::OK);

    // Delivery to the disabled subscription must 404.
    let res = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/mihomo")))
        .await
        .expect("deliver");
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

/// DEL-007 (OUT-008): `GET /sub/{token}` without a profile segment
/// auto-detects the profile from the User-Agent header.
#[tokio::test]
async fn del007_user_agent_auto_detect() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let _ = import_nodes(&router, &cookie, "trojan://pw@host-a.example.com:443#NodeA").await;
    let template_id = create_template(&router, &cookie).await;
    let v = create_sub(&router, &cookie, &template_id, "auto-sub").await;
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    // User-Agent "Clash/0.20" → Mihomo profile.
    let res = router
        .clone()
        .oneshot(get(&format!("/sub/{token}")).with_header("user-agent", "Clash/0.20".to_owned()))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::OK);
    let content_type = res
        .headers()
        .get("content-type")
        .expect("content-type")
        .to_str()
        .expect("str");
    assert!(
        content_type.contains("yaml"),
        "Clash UA should resolve to mihomo (yaml), got: {content_type}"
    );
    let content = body_to_string(res).await;
    assert!(!content.is_empty());
}

/// DEL-008 (OUT-008): `GET /sub/{token}` with an unrecognizable User-Agent
/// returns 404 (no existence leak).
#[tokio::test]
async fn del008_unknown_user_agent_returns_404() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let _ = import_nodes(&router, &cookie, "trojan://pw@host-a.example.com:443#NodeA").await;
    let template_id = create_template(&router, &cookie).await;
    let v = create_sub(&router, &cookie, &template_id, "ua-404").await;
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    let res = router
        .clone()
        .oneshot(get(&format!("/sub/{token}")).with_header("user-agent", "Mozilla/5.0".to_owned()))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

/// DEL-009: An explicit profile that does not match the subscription's
/// configured profile returns 404 (no existence leak).
#[tokio::test]
async fn del009_wrong_explicit_profile_returns_404() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let _ = import_nodes(&router, &cookie, "trojan://pw@host-a.example.com:443#NodeA").await;
    let template_id = create_template(&router, &cookie).await;
    let v = create_sub(&router, &cookie, &template_id, "profile-mismatch").await;
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    // Subscription is configured for "mihomo"; requesting "sing-box" must 404.
    let res = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/sing-box")))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

/// DEL-010: Repeated delivery requests return identical content and ETag
/// (cache hit after first generation).
#[tokio::test]
async fn del010_cache_hit_returns_same_content() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let _ = import_nodes(&router, &cookie, "trojan://pw@host-a.example.com:443#NodeA").await;
    let template_id = create_template(&router, &cookie).await;
    let v = create_sub(&router, &cookie, &template_id, "cache-sub").await;
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    let res1 = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/mihomo")))
        .await
        .expect("first");
    assert_eq!(res1.status(), StatusCode::OK);
    let etag1 = res1
        .headers()
        .get("etag")
        .expect("etag")
        .to_str()
        .expect("str")
        .to_owned();
    let content1 = body_to_string(res1).await;

    let res2 = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/mihomo")))
        .await
        .expect("second");
    assert_eq!(res2.status(), StatusCode::OK);
    let etag2 = res2
        .headers()
        .get("etag")
        .expect("etag")
        .to_str()
        .expect("str")
        .to_owned();
    let content2 = body_to_string(res2).await;

    assert_eq!(etag1, etag2, "cache hit should return same ETag");
    assert_eq!(
        content1, content2,
        "cache hit should return identical content"
    );
}

/// DEL-011: Delivery with no nodes in the pool returns 503 (generation
/// failure, constraint #19 — no fake empty config).
#[tokio::test]
async fn del011_no_nodes_returns_503() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    // No nodes imported.
    let template_id = create_template(&router, &cookie).await;
    let v = create_sub(&router, &cookie, &template_id, "empty-pool").await;
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    let res = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/mihomo")))
        .await
        .expect("send");
    assert_eq!(
        res.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "empty pool → 503, not an empty config"
    );
}

/// DEL-012 (OUT-014): Concurrent delivery requests all receive a complete,
/// identical response (no partial content from the generation pipeline).
#[tokio::test]
async fn del012_concurrent_delivery_all_complete() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let _ = import_nodes(&router, &cookie, "trojan://pw@host-a.example.com:443#NodeA").await;
    let template_id = create_template(&router, &cookie).await;
    let v = create_sub(&router, &cookie, &template_id, "concurrent").await;
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    let uri = format!("/sub/{token}/mihomo");

    // Fire 8 concurrent requests.
    let mut handles = Vec::new();
    for _ in 0..8 {
        let r = router.clone();
        let u = uri.clone();
        handles.push(tokio::spawn(async move {
            r.oneshot(get(&u)).await.expect("send")
        }));
    }
    let mut responses = Vec::new();
    for h in handles {
        responses.push(h.await.expect("join"));
    }

    let mut contents = Vec::new();
    let mut etags = Vec::new();
    for res in responses {
        assert_eq!(res.status(), StatusCode::OK);
        let etag = res
            .headers()
            .get("etag")
            .expect("etag")
            .to_str()
            .expect("str")
            .to_owned();
        let content = body_to_string(res).await;
        assert!(!content.is_empty(), "no partial/empty response");
        contents.push(content);
        etags.push(etag);
    }

    // All responses must be byte-identical (cache hit or same generation).
    let first = &contents[0];
    for (i, c) in contents.iter().enumerate() {
        assert_eq!(c, first, "response {i} differs from response 0");
    }
    let first_etag = &etags[0];
    for (i, e) in etags.iter().enumerate() {
        assert_eq!(e, first_etag, "etag {i} differs from etag 0");
    }
}
