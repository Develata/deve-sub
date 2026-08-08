#![allow(clippy::expect_used)]

//! Integration tests for subscription management endpoints (M6 Slice 1).
//!
//! Covers the full CRUD lifecycle, token rotation, slug conflict (409),
//! invalid profile (400), and SEC-009 token-plaintext redaction from
//! GET/LIST responses. See
//! `docs/plan/milestones/M6-subscription-distribution.md` Slice 1.

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

fn delete(uri: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .body(Body::empty())
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

/// Create a template via the API and return its ULID.
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

fn create_sub_body(name: &str, slug: &str, template_id: &str) -> String {
    serde_json::json!({
        "name": name,
        "slug": slug,
        "template_id": template_id,
        "profile": "mihomo",
        "node_selection": {"mode": "dynamic"},
    })
    .to_string()
}

async fn create_sub(
    router: &axum::Router,
    cookie: &str,
    template_id: &str,
    slug: &str,
) -> serde_json::Value {
    let body = create_sub_body(slug, slug, template_id);
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

/// SUB-001: Admin can create a subscription, get it, list it, update it,
/// and delete it — the full CRUD round-trip. Token plaintext is returned
/// only at creation.
#[tokio::test]
async fn sub001_subscription_crud_roundtrip() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;

    // Create.
    let v = create_sub(&router, &cookie, &template_id, "my-sub").await;
    let sub_id = v["subscription"]["id"].as_str().expect("id").to_owned();
    let token = v["token_plaintext"].as_str().expect("token").to_owned();
    // CSPRNG 32 bytes → base64url no pad = 43 chars.
    assert_eq!(token.len(), 43);
    assert_eq!(v["subscription"]["name"], "my-sub");
    assert_eq!(v["subscription"]["slug"], "my-sub");
    assert_eq!(v["subscription"]["profile"], "mihomo");
    assert_eq!(v["subscription"]["enabled"], true);
    assert!(
        v["subscription"]["owner_id"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );

    // GET the subscription — token_plaintext must NOT be present.
    let res = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/subscriptions/{sub_id}")),
            &cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::OK);
    let v = body_to_json(res).await;
    assert_eq!(v["subscription"]["id"], sub_id);
    assert!(v.get("token_plaintext").is_none());

    // List.
    let res = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/subscriptions?limit=10"), &cookie))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::OK);
    let v = body_to_json(res).await;
    assert_eq!(
        v["subscriptions"]
            .as_array()
            .expect("subscriptions array")
            .len(),
        1
    );

    // Update.
    let update = serde_json::json!({
        "name": "updated-sub",
        "slug": "updated-slug",
        "profile": "mihomo",
        "node_selection": {"mode": "dynamic"},
        "enabled": false,
    })
    .to_string();
    let res = router
        .clone()
        .oneshot(with_cookie(
            put_json(&format!("/api/v1/subscriptions/{sub_id}"), &update),
            &cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::OK);
    let v = body_to_json(res).await;
    assert_eq!(v["subscription"]["name"], "updated-sub");
    assert_eq!(v["subscription"]["slug"], "updated-slug");
    assert_eq!(v["subscription"]["enabled"], false);

    // Delete.
    let res = router
        .clone()
        .oneshot(with_cookie(
            delete(&format!("/api/v1/subscriptions/{sub_id}")),
            &cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::OK);

    // GET after delete → 404.
    let res = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/subscriptions/{sub_id}")),
            &cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

/// SUB-002: Rotating the delivery token returns a new plaintext, distinct
/// from the original, and the new token is also 43 chars.
#[tokio::test]
async fn sub002_rotate_token_returns_new_plaintext() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;

    let v = create_sub(&router, &cookie, &template_id, "rot-sub").await;
    let sub_id = v["subscription"]["id"].as_str().expect("id").to_owned();
    let original = v["token_plaintext"].as_str().expect("token").to_owned();

    let rotate = serde_json::json!({"grace_seconds": 3600}).to_string();
    let res = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                &format!("/api/v1/subscriptions/{sub_id}/rotate-token"),
                &rotate,
            ),
            &cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::OK);
    let v = body_to_json(res).await;
    let new_token = v["token_plaintext"].as_str().expect("new token").to_owned();
    assert_eq!(new_token.len(), 43);
    assert_ne!(new_token, original);
    assert!(v["token_id"].as_str().is_some());
}

/// SUB-003: Creating two subscriptions with the same slug for the same owner
/// returns 409 Conflict.
#[tokio::test]
async fn sub003_slug_conflict_returns_409() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;

    let _ = create_sub(&router, &cookie, &template_id, "dupe-slug").await;

    let body = create_sub_body("other", "dupe-slug", &template_id);
    let res = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/subscriptions", &body),
            &cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::CONFLICT);
}

/// SUB-004: An unrecognized profile returns 400 Bad Request.
#[tokio::test]
async fn sub004_unknown_profile_returns_400() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;

    let body = serde_json::json!({
        "name": "bad-profile",
        "slug": "bad-profile",
        "template_id": template_id,
        "profile": "not-a-real-profile",
        "node_selection": {"mode": "dynamic"},
    })
    .to_string();
    let res = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/subscriptions", &body),
            &cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// SEC-009 regression: the plaintext delivery token must never appear in any
/// GET or LIST response body. The token is returned only at create/rotate.
#[tokio::test]
async fn sec009_token_not_in_get_or_list_responses() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;

    let v = create_sub(&router, &cookie, &template_id, "sec-sub").await;
    let sub_id = v["subscription"]["id"].as_str().expect("id").to_owned();
    let token = v["token_plaintext"].as_str().expect("token").to_owned();

    // GET response body must not contain the token plaintext anywhere.
    let res = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/subscriptions/{sub_id}")),
            &cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::OK);
    let body_str = body_to_string(res).await;
    assert!(
        !body_str.contains(&token),
        "token plaintext must not appear in GET response"
    );

    // LIST response body must not contain the token plaintext anywhere.
    let res = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/subscriptions"), &cookie))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::OK);
    let body_str = body_to_string(res).await;
    assert!(
        !body_str.contains(&token),
        "token plaintext must not appear in LIST response"
    );
}

/// An empty name returns 400 Bad Request.
#[tokio::test]
async fn empty_name_returns_400() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;

    let body = serde_json::json!({
        "name": "",
        "slug": "empty-name",
        "template_id": template_id,
        "profile": "mihomo",
        "node_selection": {"mode": "dynamic"},
    })
    .to_string();
    let res = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/subscriptions", &body),
            &cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// An invalid cursor returns 400 Bad Request on LIST.
#[tokio::test]
async fn invalid_cursor_returns_400() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let res = router
        .clone()
        .oneshot(with_cookie(
            get("/api/v1/subscriptions?cursor=not-a-ulid"),
            &cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// An invalid subscription id in the path returns 400 Bad Request on GET.
#[tokio::test]
async fn invalid_id_returns_400() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let res = router
        .clone()
        .oneshot(with_cookie(
            get("/api/v1/subscriptions/not-a-ulid"),
            &cookie,
        ))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// Unauthenticated requests are rejected with 401.
#[tokio::test]
async fn unauthenticated_returns_401() {
    let app = TestApp::new().await;
    let router = app.router();

    let res = router
        .clone()
        .oneshot(get("/api/v1/subscriptions"))
        .await
        .expect("send");
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
