#![allow(clippy::expect_used)]

//! Integration tests for the audit log feature.
//!
//! AUDIT-001: audit log query API (filters, pagination, auth guard).
//! AUDIT-002: auth and user-management operations produce audit entries.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use deve_sub_application::{DbHealthPort, GeoIpPort, LoginRateLimiter, SubscriptionFetcher};
use deve_sub_domain::{
    AuditLogRepository, GenerationCacheRepository, LatencyProbe, LatencyRecordRepository,
    NodeOverrideRepository, NodePoolRepository, PoolMetaRepository, ProbeRunRepository,
    ProbeSourceRepository, RecoveryCodeRepository, SessionRepository, ShortCodeRepository,
    SourceRepository, SourceSnapshotRepository, SubscriptionRepository,
    SubscriptionTokenRepository, TempLinkRepository, TemplateRepository, TemplateVersionRepository,
    TotpSecretRepository, TrafficDailySnapshotRepository, TrafficRepository, UserRepository,
};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{
    SqliteAuditLogRepository, SqliteGenerationCacheRepository, SqliteHealthCheck,
    SqliteLatencyRecordRepository, SqliteNodeOverrideRepository, SqliteNodePoolRepository,
    SqlitePoolMetaRepository, SqliteProbeRunRepository, SqliteProbeSourceRepository,
    SqliteRecoveryCodeRepository, SqliteSessionRepository, SqliteShortCodeRepository,
    SqliteSourceRepository, SqliteSourceSnapshotRepository, SqliteSubscriptionRepository,
    SqliteSubscriptionTokenRepository, SqliteTempLinkRepository, SqliteTemplateRepository,
    SqliteTemplateVersionRepository, SqliteTotpSecretRepository,
    SqliteTrafficDailySnapshotRepository, SqliteTrafficRepository, SqliteUserRepository,
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
                master_key: Arc::clone(&master_key),
                audit_log_repo: Arc::new(SqliteAuditLogRepository::new(pool.clone()))
                    as Arc<dyn AuditLogRepository>,
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
                traffic_daily_snapshot_repo: Arc::new(SqliteTrafficDailySnapshotRepository::new(
                    pool.clone(),
                ))
                    as Arc<dyn TrafficDailySnapshotRepository>,
                probe_source_repo: Arc::new(SqliteProbeSourceRepository::new(pool.clone()))
                    as Arc<dyn ProbeSourceRepository>,
                probe_run_repo: Arc::new(SqliteProbeRunRepository::new(pool.clone()))
                    as Arc<dyn ProbeRunRepository>,
                latency_repo: Arc::new(SqliteLatencyRecordRepository::new(pool.clone()))
                    as Arc<dyn LatencyRecordRepository>,
                probe_adapter: std::sync::Arc::new(
                    deve_sub_adapters::ProbeSourceAdapterRegistry::new()
                        .with_nezha(std::sync::Arc::new(
                            deve_sub_adapters::NezhaProbeAdapter::new(std::sync::Arc::clone(
                                &master_key,
                            )),
                        ))
                        .with_dstatus(std::sync::Arc::new(
                            deve_sub_adapters::DStatusProbeAdapter::new(std::sync::Arc::clone(
                                &master_key,
                            )),
                        ))
                        .with_komari(std::sync::Arc::new(
                            deve_sub_adapters::KomariProbeAdapter::new(std::sync::Arc::clone(
                                &master_key,
                            )),
                        )),
                )
                    as std::sync::Arc<dyn deve_sub_domain::ProbeSourceAdapter>,
                tcp_probe: Arc::new(deve_sub_adapters::TcpConnectProbe::new())
                    as Arc<dyn LatencyProbe>,
                quic_probe: Arc::new(deve_sub_adapters::QuicHandshakeProbe::new())
                    as Arc<dyn LatencyProbe>,
                real_proxy_probe: Arc::new(deve_sub_adapters::RealProxyProbe::new())
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

/// Helper: setup admin, login, return (router, cookie, admin_id).
async fn setup_and_login(app: &TestApp) -> (axum::Router, String, String) {
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
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = extract_cookie(&response).expect("cookie");
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let admin_id = json["user"]["id"].as_str().expect("admin id").to_owned();
    (router, cookie, admin_id)
}

/// AUDIT-001: Audit log query API returns entries and supports filters.
#[tokio::test]
async fn audit_log_query_returns_entries() {
    let app = TestApp::new().await;
    let (router, cookie, _admin_id) = setup_and_login(&app).await;

    // The login above should have produced an auth.login audit entry.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/audit-logs", &cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let entries = json["entries"].as_array().expect("entries array");
    assert!(!entries.is_empty(), "at least one audit entry expected");
    let has_login = entries.iter().any(|e| e["action"] == "auth.login");
    assert!(has_login, "auth.login entry should exist");
}

/// AUDIT-001: action filter narrows results.
#[tokio::test]
async fn audit_log_query_filter_by_action() {
    let app = TestApp::new().await;
    let (router, cookie, _admin_id) = setup_and_login(&app).await;

    let response = router
        .clone()
        .oneshot(get_with_cookie(
            "/api/v1/audit-logs?action=auth.login",
            &cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let entries = json["entries"].as_array().expect("entries array");
    assert!(!entries.is_empty());
    for entry in entries {
        assert_eq!(entry["action"], "auth.login");
    }
}

/// AUDIT-001: actor_id filter narrows results.
#[tokio::test]
async fn audit_log_query_filter_by_actor() {
    let app = TestApp::new().await;
    let (router, cookie, admin_id) = setup_and_login(&app).await;

    let uri = format!("/api/v1/audit-logs?actor_id={admin_id}");
    let response = router
        .clone()
        .oneshot(get_with_cookie(&uri, &cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let entries = json["entries"].as_array().expect("entries array");
    assert!(!entries.is_empty());
    for entry in entries {
        assert_eq!(entry["actor_id"], admin_id);
    }
}

/// AUDIT-001: pagination via limit + cursor.
#[tokio::test]
async fn audit_log_query_pagination() {
    let app = TestApp::new().await;
    let (router, cookie, _admin_id) = setup_and_login(&app).await;

    // Logout and login again to generate at least 2+ entries.
    let _ = router
        .clone()
        .oneshot(post_with_cookie("/api/v1/auth/logout", "{}", &cookie))
        .await
        .expect("logout");
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("login");
    let cookie2 = extract_cookie(&response).expect("cookie");

    // Page 1 with limit=1.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/audit-logs?limit=1", &cookie2))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let entries = json["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1, "limit=1 should return exactly 1 entry");
    let cursor = json["next_cursor"]
        .as_str()
        .expect("next_cursor should exist when more entries remain");

    // Page 2 using the cursor.
    let uri = format!("/api/v1/audit-logs?limit=1&cursor={cursor}");
    let response = router
        .clone()
        .oneshot(get_with_cookie(&uri, &cookie2))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let entries = json["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1, "page 2 should also return 1 entry");
}

/// AUDIT-001: unauthenticated request returns 401.
#[tokio::test]
async fn audit_log_query_requires_auth() {
    let app = TestApp::new().await;
    let router = app.router();

    let response = router
        .oneshot(get("/api/v1/audit-logs"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// AUDIT-001: non-admin user gets 403.
#[tokio::test]
async fn audit_log_query_requires_admin() {
    let app = TestApp::new().await;
    let (router, cookie, _admin_id) = setup_and_login(&app).await;

    // Create a regular (non-admin) user.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/users",
            r#"{"username":"regular","password":"s3cure-pwd!","role":"user"}"#,
            &cookie,
        ))
        .await
        .expect("create user");
    assert_eq!(response.status(), StatusCode::CREATED);

    // Login as the regular user.
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"regular","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("login");
    assert_eq!(response.status(), StatusCode::OK);
    let user_cookie = extract_cookie(&response).expect("cookie");

    // Non-admin querying audit logs → 403.
    let response = router
        .oneshot(get_with_cookie("/api/v1/audit-logs", &user_cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// AUDIT-002: login and logout produce audit entries.
#[tokio::test]
async fn auth_login_logout_audited() {
    let app = TestApp::new().await;
    let (router, cookie, admin_id) = setup_and_login(&app).await;

    // Logout to generate auth.logout entry.
    let _ = router
        .clone()
        .oneshot(post_with_cookie("/api/v1/auth/logout", "{}", &cookie))
        .await
        .expect("logout");

    // Login again to get a fresh session for querying.
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"admin","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("login");
    let cookie2 = extract_cookie(&response).expect("cookie");

    // Query all audit entries.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/audit-logs", &cookie2))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 16384)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let entries = json["entries"].as_array().expect("entries array");

    let actions: Vec<&str> = entries
        .iter()
        .map(|e| e["action"].as_str().expect("action"))
        .collect();
    assert!(
        actions.contains(&"auth.login"),
        "auth.login should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"auth.logout"),
        "auth.logout should be audited: {actions:?}"
    );

    // All entries should have the admin's actor_id.
    for entry in entries {
        assert_eq!(entry["actor_id"], admin_id);
    }
}

/// AUDIT-002: user create, disable, and force-logout produce audit entries.
#[tokio::test]
async fn user_management_audited() {
    let app = TestApp::new().await;
    let (router, cookie, _admin_id) = setup_and_login(&app).await;

    // Create a user — should produce user.create entry.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/users",
            r#"{"username":"alice","password":"s3cure-pwd!","role":"user"}"#,
            &cookie,
        ))
        .await
        .expect("create user");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let alice_id = json["user"]["id"].as_str().expect("alice id").to_owned();

    // Login as alice so she has a session, then force-logout her.
    let _ = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"alice","password":"s3cure-pwd!"}"#,
        ))
        .await
        .expect("alice login");

    let response = router
        .clone()
        .oneshot(post_with_cookie(
            &format!("/api/v1/users/{alice_id}/force-logout"),
            "{}",
            &cookie,
        ))
        .await
        .expect("force logout");
    assert_eq!(response.status(), StatusCode::OK);

    // Create a second user to disable.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/users",
            r#"{"username":"bob","password":"s3cure-pwd!","role":"user"}"#,
            &cookie,
        ))
        .await
        .expect("create bob");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let bob_id = json["user"]["id"].as_str().expect("bob id").to_owned();

    let response = router
        .clone()
        .oneshot(post_with_cookie(
            &format!("/api/v1/users/{bob_id}/disable"),
            "{}",
            &cookie,
        ))
        .await
        .expect("disable bob");
    assert_eq!(response.status(), StatusCode::OK);

    // Query audit logs and verify user.create, user.disable, user.force_logout.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/audit-logs", &cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 32768)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let entries = json["entries"].as_array().expect("entries array");

    let actions: Vec<&str> = entries
        .iter()
        .map(|e| e["action"].as_str().expect("action"))
        .collect();
    assert!(
        actions.contains(&"user.create"),
        "user.create should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"user.disable"),
        "user.disable should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"user.force_logout"),
        "user.force_logout should be audited: {actions:?}"
    );

    // Verify target_id on the user.create entry for alice.
    let alice_create = entries
        .iter()
        .find(|e| e["action"] == "user.create" && e["target_id"] == alice_id);
    assert!(alice_create.is_some(), "user.create for alice should exist");
    if let Some(entry) = alice_create {
        assert_eq!(entry["target_type"], "user");
    }
}
