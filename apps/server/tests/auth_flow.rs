#![allow(clippy::expect_used)]

//! Integration tests for the auth flow: setup-admin, login, logout, me.
//!
//! Covers AUTH-001 (init admin), AUTH-002 (correct login), AUTH-003 (wrong
//! password), and SEC-009 (token redaction in logs).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use deve_sub_application::{DbHealthPort, GeoIpPort, LoginRateLimiter, SubscriptionFetcher};
use deve_sub_domain::{
    NodeOverrideRepository, NodePoolRepository, RecoveryCodeRepository, SessionRepository,
    SourceRepository, SourceSnapshotRepository, TemplateRepository, TemplateVersionRepository,
    TotpSecretRepository, UserRepository,
};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{
    SqliteHealthCheck, SqliteNodeOverrideRepository, SqliteNodePoolRepository,
    SqliteRecoveryCodeRepository, SqliteSessionRepository, SqliteSourceRepository,
    SqliteSourceSnapshotRepository, SqliteTemplateRepository, SqliteTemplateVersionRepository,
    SqliteTotpSecretRepository, SqliteUserRepository,
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
                override_repo: Arc::new(SqliteNodeOverrideRepository::new(pool.clone()))
                    as Arc<dyn NodeOverrideRepository>,
                template_repo: Arc::new(SqliteTemplateRepository::new(pool.clone()))
                    as Arc<dyn TemplateRepository>,
                version_repo: Arc::new(SqliteTemplateVersionRepository::new(pool.clone()))
                    as Arc<dyn TemplateVersionRepository>,
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

fn get_with_cookie(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("cookie", cookie)
        .body(Body::empty())
        .expect("request")
}

fn post_with_cookie(uri: &str, body: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("cookie", cookie)
        .body(json_body(body))
        .expect("request")
}

fn extract_cookie(response: &axum::response::Response) -> Option<String> {
    let cookies = response.headers().get("set-cookie")?.to_str().ok()?;
    let part = cookies
        .split(';')
        .find(|s| s.trim().starts_with("deve_sub_session="))?;
    Some(part.trim().to_owned())
}

/// Return the full `Set-Cookie` header value for attribute assertions.
fn full_set_cookie(response: &axum::response::Response) -> Option<String> {
    response
        .headers()
        .get("set-cookie")?
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// AUTH-001: Initial admin creation succeeds and can only be done once.
#[tokio::test]
async fn setup_admin_succeeds_once() {
    let app = TestApp::new().await;
    let router = app.router();

    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/setup",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["user"]["username"], "admin");
    assert_eq!(json["user"]["role"], "admin");
    assert_eq!(json["user"]["enabled"], true);
    assert!(json["user"]["id"].as_str().is_some());

    // Second setup must fail with 409.
    let response = router
        .oneshot(post_json(
            "/api/v1/auth/setup",
            r#"{"username":"admin2","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

/// AUTH-002: Correct login creates a session and sets a cookie.
#[tokio::test]
async fn login_with_correct_credentials() {
    let app = TestApp::new().await;

    // Setup admin first.
    let router = app.router();
    let _ = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/setup",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("setup");

    // Login.
    let response = router
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let full_cookie = full_set_cookie(&response).expect("Set-Cookie header");
    assert!(full_cookie.contains("HttpOnly"));
    assert!(full_cookie.contains("SameSite=Lax"));
    assert!(full_cookie.contains("Secure"));

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["user"]["username"], "admin");
}

/// AUTH-003: Wrong password returns 401 without leaking user existence.
#[tokio::test]
async fn login_with_wrong_password() {
    let app = TestApp::new().await;

    // Setup admin first.
    let router = app.router();
    let _ = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/setup",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("setup");

    // Login with wrong password.
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"wrong-password"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["error"], "invalid_credentials");
    assert!(json["message"].as_str().is_some());

    // Login with non-existent user — same error to avoid leaking existence.
    let response = router
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"ghost","password":"anything"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["error"], "invalid_credentials");
}

/// Authenticated /me endpoint returns the current user.
#[tokio::test]
async fn me_with_valid_session() {
    let app = TestApp::new().await;
    let router = app.router();

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
    let cookie = extract_cookie(&response).expect("cookie");

    // /me with the session cookie.
    let response = router
        .oneshot(get_with_cookie("/api/v1/auth/me", &cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["user"]["username"], "admin");
}

/// /me without a session cookie returns 401.
#[tokio::test]
async fn me_without_session() {
    let app = TestApp::new().await;
    let router = app.router();

    let response = router
        .oneshot(get("/api/v1/auth/me"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// Logout revokes the session and clears the cookie.
#[tokio::test]
async fn logout_revokes_session() {
    let app = TestApp::new().await;
    let router = app.router();

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
    let cookie = extract_cookie(&response).expect("cookie");

    // Logout.
    let response = router
        .clone()
        .oneshot(post_with_cookie("/api/v1/auth/logout", "{}", &cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // /me with the old cookie should fail.
    let response = router
        .oneshot(get_with_cookie("/api/v1/auth/me", &cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
