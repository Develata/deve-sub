#![allow(clippy::expect_used)]

//! Integration tests for login rate limiting (AUTH-004) and CSRF
//! protection (SEC-010).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use deve_sub_application::{DbHealthPort, GeoIpPort, LoginRateLimiter, SubscriptionFetcher};
use deve_sub_domain::{
    GenerationCacheRepository, NodeOverrideRepository, NodePoolRepository, PoolMetaRepository,
    RecoveryCodeRepository, SessionRepository, SourceRepository, SourceSnapshotRepository,
    TemplateRepository, TemplateVersionRepository, TotpSecretRepository, UserRepository,
};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{
    SqliteGenerationCacheRepository, SqliteHealthCheck, SqliteNodeOverrideRepository,
    SqliteNodePoolRepository, SqlitePoolMetaRepository, SqliteRecoveryCodeRepository,
    SqliteSessionRepository, SqliteSourceRepository, SqliteSourceSnapshotRepository,
    SqliteTemplateRepository, SqliteTemplateVersionRepository, SqliteTotpSecretRepository,
    SqliteUserRepository,
};

struct TestApp {
    state: deve_sub_server::AppState,
    _dir: tempfile::TempDir,
}

impl TestApp {
    async fn new() -> Self {
        Self::with_max_attempts(5).await
    }

    /// Create a test app with a custom rate limit threshold.
    async fn with_max_attempts(max_attempts: u32) -> Self {
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

        let mut config = deve_sub_application::AppConfig::default();
        config.security.max_login_attempts = max_attempts;
        config.security.lockout_duration_secs = 300;

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
        .header("host", "localhost:8080")
        .body(json_body(body))
        .expect("request")
}

fn post_json_with_origin(uri: &str, body: &str, origin: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("host", "localhost:8080")
        .header("origin", origin)
        .body(json_body(body))
        .expect("request")
}

fn post_json_with_xff(uri: &str, body: &str, ip: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("host", "localhost:8080")
        .header("x-forwarded-for", ip)
        .body(json_body(body))
        .expect("request")
}

/// Set up an admin and return the session cookie.
async fn setup_admin(router: &axum::Router) {
    let _ = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/setup",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("setup");
}

/// AUTH-004: Repeated failed logins temporarily lock the account.
#[tokio::test]
async fn login_rate_limited_after_threshold() {
    let app = TestApp::with_max_attempts(3).await;
    let router = app.router();
    setup_admin(&router).await;

    // Two failed attempts — should still be allowed (401).
    for _ in 0..2 {
        let response = router
            .clone()
            .oneshot(post_json(
                "/api/v1/auth/login",
                r#"{"username":"admin","password":"wrong"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // Third failed attempt — this one triggers the lock.
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"wrong"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Fourth attempt — now locked (429).
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"wrong"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["error"], "rate_limited");

    // Even with correct credentials, the locked account is rejected (429).
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

/// AUTH-004: Rate limiting applies to non-existent usernames too (no
/// enumeration via rate-limiting behavior).
#[tokio::test]
async fn rate_limiting_applies_to_nonexistent_user() {
    let app = TestApp::with_max_attempts(3).await;
    let router = app.router();
    setup_admin(&router).await;

    // Three failed attempts on a non-existent username.
    for _ in 0..3 {
        let response = router
            .clone()
            .oneshot(post_json(
                "/api/v1/auth/login",
                r#"{"username":"ghost","password":"wrong"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // Fourth attempt — now locked (429), same as a real user.
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"ghost","password":"wrong"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

/// AUTH-004: Successful login resets the failure counter.
#[tokio::test]
async fn successful_login_resets_counter() {
    let app = TestApp::with_max_attempts(3).await;
    let router = app.router();
    setup_admin(&router).await;

    // Two failed attempts.
    for _ in 0..2 {
        let _ = router
            .clone()
            .oneshot(post_json(
                "/api/v1/auth/login",
                r#"{"username":"admin","password":"wrong"}"#,
            ))
            .await
            .expect("response");
    }

    // Successful login resets the counter.
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // Two more failed attempts — should still be allowed (not locked).
    for _ in 0..2 {
        let response = router
            .clone()
            .oneshot(post_json(
                "/api/v1/auth/login",
                r#"{"username":"admin","password":"wrong"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

/// AUTH-004: Rate limiting is per-IP — a locked IP blocks all usernames
/// from that IP.
#[tokio::test]
async fn rate_limiting_per_ip() {
    let app = TestApp::with_max_attempts(3).await;
    let router = app.router();
    setup_admin(&router).await;

    // Three failed attempts from IP 10.0.0.1 as "admin".
    for _ in 0..3 {
        let _ = router
            .clone()
            .oneshot(post_json_with_xff(
                "/api/v1/auth/login",
                r#"{"username":"admin","password":"wrong"}"#,
                "10.0.0.1",
            ))
            .await
            .expect("response");
    }

    // From the same IP, even a different username is locked.
    let response = router
        .clone()
        .oneshot(post_json_with_xff(
            "/api/v1/auth/login",
            r#"{"username":"ghost","password":"wrong"}"#,
            "10.0.0.1",
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    // From a different IP, the username "admin" is still locked — it
    // accumulated 3 failures and the username counter is IP-independent.
    let response = router
        .clone()
        .oneshot(post_json_with_xff(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"wrong"}"#,
            "10.0.0.2",
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    // But "ghost" from a different IP is fine.
    let response = router
        .clone()
        .oneshot(post_json_with_xff(
            "/api/v1/auth/login",
            r#"{"username":"ghost","password":"wrong"}"#,
            "10.0.0.2",
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// SEC-010: Cross-origin POST requests are rejected by CSRF middleware.
#[tokio::test]
async fn csrf_rejects_cross_origin_post() {
    let app = TestApp::new().await;
    let router = app.router();
    setup_admin(&router).await;

    // POST with a mismatched Origin header → 403.
    let response = router
        .clone()
        .oneshot(post_json_with_origin(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
            "https://evil.com",
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["error"], "csrf_error");
}

/// SEC-010: Same-origin POST requests are allowed.
#[tokio::test]
async fn csrf_allows_same_origin_post() {
    let app = TestApp::new().await;
    let router = app.router();
    setup_admin(&router).await;

    // POST with a matching Origin header → allowed.
    let response = router
        .clone()
        .oneshot(post_json_with_origin(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
            "http://localhost:8080",
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

/// SEC-010: POST without an Origin header is allowed (SameSite=Lax
/// provides the primary CSRF defense; non-browser clients don't send
/// Origin).
#[tokio::test]
async fn csrf_allows_post_without_origin() {
    let app = TestApp::new().await;
    let router = app.router();
    setup_admin(&router).await;

    // POST without Origin header → allowed.
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

/// SEC-010: GET requests are not subject to CSRF validation.
#[tokio::test]
async fn csrf_does_not_apply_to_get() {
    let app = TestApp::new().await;
    let router = app.router();

    // GET with a mismatched Origin — should not be rejected (GET is
    // idempotent, not state-changing).
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health/live")
                .header("host", "localhost:8080")
                .header("origin", "https://evil.com")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}
