#![allow(clippy::expect_used)]

//! Integration tests for 2FA (TOTP + recovery codes).
//!
//! - AUTH-005: 2FA setup → verify → login flow completes end-to-end.
//! - AUTH-006: Recovery codes are single-use.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use deve_sub_application::{DbHealthPort, GeoIpPort, LoginRateLimiter, SubscriptionFetcher};
use deve_sub_domain::{
    GenerationCacheRepository, LatencyProbe, LatencyRecordRepository, NodeOverrideRepository,
    NodePoolRepository, PoolMetaRepository, ProbeRunRepository, ProbeSourceRepository,
    RecoveryCodeRepository, SessionRepository, ShortCodeRepository, SourceRepository,
    SourceSnapshotRepository, SubscriptionRepository, SubscriptionTokenRepository,
    TempLinkRepository, TemplateRepository, TemplateVersionRepository, TotpSecretRepository,
    TrafficRepository, UserRepository,
};
use deve_sub_security::{MasterKey, base32_decode, totp_generate_code};
use deve_sub_storage_sqlite::{
    SqliteGenerationCacheRepository, SqliteHealthCheck, SqliteLatencyRecordRepository,
    SqliteNodeOverrideRepository, SqliteNodePoolRepository, SqlitePoolMetaRepository,
    SqliteProbeRunRepository, SqliteProbeSourceRepository, SqliteRecoveryCodeRepository,
    SqliteSessionRepository, SqliteShortCodeRepository, SqliteSourceRepository,
    SqliteSourceSnapshotRepository, SqliteSubscriptionRepository,
    SqliteSubscriptionTokenRepository, SqliteTempLinkRepository, SqliteTemplateRepository,
    SqliteTemplateVersionRepository, SqliteTotpSecretRepository, SqliteTrafficRepository,
    SqliteUserRepository,
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
                short_code_repo: Arc::new(SqliteShortCodeRepository::new(pool.clone()))
                    as Arc<dyn ShortCodeRepository>,
                temp_link_repo: Arc::new(SqliteTempLinkRepository::new(pool.clone()))
                    as Arc<dyn TempLinkRepository>,
                traffic_repo: Arc::new(SqliteTrafficRepository::new(pool.clone()))
                    as Arc<dyn TrafficRepository>,
                probe_source_repo: Arc::new(SqliteProbeSourceRepository::new(pool.clone()))
                    as Arc<dyn ProbeSourceRepository>,
                probe_run_repo: Arc::new(SqliteProbeRunRepository::new(pool.clone()))
                    as Arc<dyn ProbeRunRepository>,
                latency_repo: Arc::new(SqliteLatencyRecordRepository::new(pool.clone()))
                    as Arc<dyn LatencyRecordRepository>,
                tcp_probe: Arc::new(deve_sub_adapters::TcpConnectProbe::new())
                    as Arc<dyn LatencyProbe>,
                quic_probe: Arc::new(deve_sub_adapters::QuicHandshakeProbe::new())
                    as Arc<dyn LatencyProbe>,
                cancelled_flags: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
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

fn post_json_with_cookie(uri: &str, body: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("host", "localhost:8080")
        .header("cookie", cookie)
        .body(json_body(body))
        .expect("request")
}

fn get_with_cookie(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("host", "localhost:8080")
        .header("cookie", cookie)
        .body(Body::empty())
        .expect("request")
}

/// Extract the session cookie from a response's `set-cookie` header.
fn extract_cookie(response: &axum::http::Response<Body>) -> String {
    let set_cookie = response
        .headers()
        .get("set-cookie")
        .expect("set-cookie header")
        .to_str()
        .expect("ascii");
    // The cookie is `deve_sub_session=<token>; HttpOnly; ...` — extract just
    // the `name=value` part.
    let cookie_value = set_cookie.split(';').next().expect("cookie value");
    cookie_value.to_owned()
}

/// Setup an admin user and return the session cookie.
async fn setup_admin_and_login(router: &axum::Router) -> String {
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
    extract_cookie(&response)
}

/// Setup 2FA for the admin user. Returns (cookie, recovery_codes, totp_secret_bytes).
async fn setup_2fa(router: &axum::Router, cookie: &str) -> (Vec<String>, Vec<u8>) {
    // Step 1: setup — get TOTP secret
    let response = router
        .clone()
        .oneshot(post_json_with_cookie(
            "/api/v1/auth/2fa/setup",
            r#"{}"#,
            cookie,
        ))
        .await
        .expect("setup");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let secret_b32 = json["secret"].as_str().expect("secret");
    let secret_bytes = base32_decode(secret_b32).expect("decode base32");

    // Step 2: verify — generate TOTP code and enable 2FA
    let code = totp_generate_code(&secret_bytes);
    let verify_body = format!(r#"{{"code":"{code:06}"}}"#);

    let response = router
        .clone()
        .oneshot(post_json_with_cookie(
            "/api/v1/auth/2fa/verify",
            &verify_body,
            cookie,
        ))
        .await
        .expect("verify");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let recovery_codes: Vec<String> = json["recovery_codes"]
        .as_array()
        .expect("recovery_codes array")
        .iter()
        .map(|v| v.as_str().expect("string").to_owned())
        .collect();

    assert!(!recovery_codes.is_empty());
    (recovery_codes, secret_bytes)
}

/// AUTH-005: Full 2FA flow — setup, verify, login with TOTP code.
#[tokio::test]
async fn twofa_login_with_totp_code() {
    let app = TestApp::new().await;
    let router = app.router();

    let cookie = setup_admin_and_login(&router).await;
    let (_recovery_codes, secret_bytes) = setup_2fa(&router, &cookie).await;

    // Logout
    let response = router
        .clone()
        .oneshot(post_json_with_cookie(
            "/api/v1/auth/logout",
            r#"{}"#,
            &cookie,
        ))
        .await
        .expect("logout");
    assert_eq!(response.status(), StatusCode::OK);

    // Login again — should get 2FA challenge
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("login");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["requires_2fa"], true);
    let challenge_token = json["challenge_token"]
        .as_str()
        .expect("challenge_token")
        .to_owned();

    // No session cookie should be set
    assert!(
        json.get("user").is_some(),
        "user should be returned with 2FA challenge"
    );

    // Complete 2FA login with TOTP code
    let code = totp_generate_code(&secret_bytes);
    let login_2fa_body = format!(r#"{{"challenge_token":"{challenge_token}","code":"{code:06}"}}"#);

    let response = router
        .clone()
        .oneshot(post_json("/api/v1/auth/login/2fa", &login_2fa_body))
        .await
        .expect("2fa login");
    assert_eq!(response.status(), StatusCode::OK);

    let session_cookie = extract_cookie(&response);

    // Verify the session works
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/auth/me", &session_cookie))
        .await
        .expect("me");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["user"]["username"], "admin");
    assert_eq!(json["user"]["two_factor_enabled"], true);
}

/// AUTH-005: 2FA login with wrong TOTP code returns 401.
#[tokio::test]
async fn twofa_login_wrong_totp_code() {
    let app = TestApp::new().await;
    let router = app.router();

    let cookie = setup_admin_and_login(&router).await;
    let (_recovery_codes, _secret_bytes) = setup_2fa(&router, &cookie).await;

    // Logout
    let _ = router
        .clone()
        .oneshot(post_json_with_cookie(
            "/api/v1/auth/logout",
            r#"{}"#,
            &cookie,
        ))
        .await;

    // Login → 2FA challenge
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("login");

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let challenge_token = json["challenge_token"]
        .as_str()
        .expect("challenge_token")
        .to_owned();

    // Wrong TOTP code
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login/2fa",
            &format!(r#"{{"challenge_token":"{challenge_token}","code":"000000"}}"#),
        ))
        .await
        .expect("2fa login");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// AUTH-005: 2FA login with invalid challenge token returns 401.
#[tokio::test]
async fn twofa_login_invalid_challenge_token() {
    let app = TestApp::new().await;
    let router = app.router();

    let _ = setup_admin_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login/2fa",
            r#"{"challenge_token":"invalid.token","code":"123456"}"#,
        ))
        .await
        .expect("2fa login");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// AUTH-006: Recovery codes are single-use.
#[tokio::test]
async fn recovery_code_single_use() {
    let app = TestApp::new().await;
    let router = app.router();

    let cookie = setup_admin_and_login(&router).await;
    let (recovery_codes, _secret_bytes) = setup_2fa(&router, &cookie).await;

    // Logout
    let _ = router
        .clone()
        .oneshot(post_json_with_cookie(
            "/api/v1/auth/logout",
            r#"{}"#,
            &cookie,
        ))
        .await;

    // Login → 2FA challenge
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("login");

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let challenge_token = json["challenge_token"]
        .as_str()
        .expect("challenge_token")
        .to_owned();

    // Use first recovery code — should succeed
    let first_code = &recovery_codes[0];
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login/2fa",
            &format!(r#"{{"challenge_token":"{challenge_token}","code":"{first_code}"}}"#),
        ))
        .await
        .expect("recovery login");
    assert_eq!(response.status(), StatusCode::OK);
    let session_cookie = extract_cookie(&response);

    // Verify the session works
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/auth/me", &session_cookie))
        .await
        .expect("me");
    assert_eq!(response.status(), StatusCode::OK);

    // Logout
    let _ = router
        .clone()
        .oneshot(post_json_with_cookie(
            "/api/v1/auth/logout",
            r#"{}"#,
            &session_cookie,
        ))
        .await;

    // Login again → new 2FA challenge
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("login");

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let challenge_token = json["challenge_token"]
        .as_str()
        .expect("challenge_token")
        .to_owned();

    // Try the SAME recovery code again — should fail (single-use)
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login/2fa",
            &format!(r#"{{"challenge_token":"{challenge_token}","code":"{first_code}"}}"#),
        ))
        .await
        .expect("recovery reuse");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Use a SECOND recovery code — should succeed
    let second_code = &recovery_codes[1];
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login/2fa",
            &format!(r#"{{"challenge_token":"{challenge_token}","code":"{second_code}"}}"#),
        ))
        .await
        .expect("second recovery");
    assert_eq!(response.status(), StatusCode::OK);
}

/// 2FA setup when already enabled returns 409.
#[tokio::test]
async fn setup_twofa_when_already_enabled() {
    let app = TestApp::new().await;
    let router = app.router();

    let cookie = setup_admin_and_login(&router).await;
    let (_recovery_codes, _secret) = setup_2fa(&router, &cookie).await;

    // Try to setup again — should get 409
    let response = router
        .clone()
        .oneshot(post_json_with_cookie(
            "/api/v1/auth/2fa/setup",
            r#"{}"#,
            &cookie,
        ))
        .await
        .expect("setup");
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

/// 2FA disable flow.
#[tokio::test]
async fn disable_twofa() {
    let app = TestApp::new().await;
    let router = app.router();

    let cookie = setup_admin_and_login(&router).await;
    let (_recovery_codes, _secret) = setup_2fa(&router, &cookie).await;

    // Disable 2FA with correct password
    let response = router
        .clone()
        .oneshot(post_json_with_cookie(
            "/api/v1/auth/2fa/disable",
            r#"{"password":"s3cure-pwd!"}"#,
            &cookie,
        ))
        .await
        .expect("disable");
    assert_eq!(response.status(), StatusCode::OK);

    // Login should now work without 2FA
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("login");

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["requires_2fa"], false);
}

/// 2FA disable with wrong password returns 401.
#[tokio::test]
async fn disable_twofa_wrong_password() {
    let app = TestApp::new().await;
    let router = app.router();

    let cookie = setup_admin_and_login(&router).await;
    let (_recovery_codes, _secret) = setup_2fa(&router, &cookie).await;

    let response = router
        .clone()
        .oneshot(post_json_with_cookie(
            "/api/v1/auth/2fa/disable",
            r#"{"password":"wrong-password"}"#,
            &cookie,
        ))
        .await
        .expect("disable");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// Regenerate recovery codes.
#[tokio::test]
async fn regenerate_recovery_codes() {
    let app = TestApp::new().await;
    let router = app.router();

    let cookie = setup_admin_and_login(&router).await;
    let (original_codes, _secret) = setup_2fa(&router, &cookie).await;

    // Regenerate recovery codes
    let response = router
        .clone()
        .oneshot(post_json_with_cookie(
            "/api/v1/auth/2fa/recovery-codes",
            r#"{"password":"s3cure-pwd!"}"#,
            &cookie,
        ))
        .await
        .expect("regenerate");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let new_codes: Vec<String> = json["recovery_codes"]
        .as_array()
        .expect("recovery_codes")
        .iter()
        .map(|v| v.as_str().expect("string").to_owned())
        .collect();

    assert!(!new_codes.is_empty());
    // New codes should be different from original
    assert_ne!(new_codes, original_codes);

    // Logout and login → 2FA challenge
    let _ = router
        .clone()
        .oneshot(post_json_with_cookie(
            "/api/v1/auth/logout",
            r#"{}"#,
            &cookie,
        ))
        .await;

    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("login");

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let challenge_token = json["challenge_token"]
        .as_str()
        .expect("challenge_token")
        .to_owned();

    // Old recovery code should NOT work
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login/2fa",
            &format!(
                r#"{{"challenge_token":"{challenge_token}","code":"{}"}}"#,
                original_codes[0]
            ),
        ))
        .await
        .expect("old code");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // New recovery code should work
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login/2fa",
            &format!(
                r#"{{"challenge_token":"{challenge_token}","code":"{}"}}"#,
                new_codes[0]
            ),
        ))
        .await
        .expect("new code");
    assert_eq!(response.status(), StatusCode::OK);
}
