#![allow(clippy::expect_used)]

//! Integration tests for source management endpoints (SRC-001).
//!
//! Covers the full CRUD lifecycle: create, list, get, update, delete, plus
//! authorization (admin-only) and validation errors. See
//! `docs/plan/milestones/M4-sources-and-node-pool.md` Slice 1.

use std::str::FromStr;
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
use deve_sub_security::MasterKey;
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
                master_key: Arc::clone(&master_key),
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
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    serde_json::from_slice(&body).expect("json")
}

const VALID_SOURCE_BODY: &str = r#"{"name":"my-sub","source_type":"auto","url":"https://example.com/sub","auto_update":true,"update_interval_secs":1800,"keep_on_fail":true}"#;

/// SRC-001: Admin can create a source and get it back.
#[tokio::test]
async fn admin_can_create_source() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", VALID_SOURCE_BODY),
            &cookie,
        ))
        .await
        .expect("create");

    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    assert_eq!(json["source"]["name"], "my-sub");
    assert_eq!(json["source"]["source_type"], "auto");
    assert_eq!(json["source"]["url"], "https://example.com/sub");
    assert_eq!(json["source"]["auto_update"], true);
    assert_eq!(json["source"]["update_interval_secs"], 1800);
    assert_eq!(json["source"]["enabled"], true);
    assert_eq!(json["source"]["keep_on_fail"], true);
    assert!(json["source"]["id"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(
        json["source"]["created_at"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
}

/// SRC-001: Unauthenticated requests are rejected with 401.
#[tokio::test]
async fn unauthenticated_rejected() {
    let app = TestApp::new().await;
    let router = app.router();

    let response = router
        .clone()
        .oneshot(post_json("/api/v1/sources", VALID_SOURCE_BODY))
        .await
        .expect("create");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// SRC-001: Duplicate name returns 409.
#[tokio::test]
async fn duplicate_name_returns_conflict() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let r1 = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", VALID_SOURCE_BODY),
            &cookie,
        ))
        .await
        .expect("first");
    assert_eq!(r1.status(), StatusCode::CREATED);

    let r2 = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", VALID_SOURCE_BODY),
            &cookie,
        ))
        .await
        .expect("second");
    assert_eq!(r2.status(), StatusCode::CONFLICT);
}

/// SRC-001: Invalid input (empty name) returns 400.
#[tokio::test]
async fn empty_name_rejected() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/sources",
                r#"{"name":"","source_type":"auto","url":"https://example.com/sub"}"#,
            ),
            &cookie,
        ))
        .await
        .expect("create");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// SRC-001: Non-HTTP URL returns 400.
#[tokio::test]
async fn non_http_url_rejected() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/sources",
                r#"{"name":"bad","source_type":"auto","url":"ftp://example.com/sub"}"#,
            ),
            &cookie,
        ))
        .await
        .expect("create");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// SRC-001: Admin can list sources with pagination.
#[tokio::test]
async fn admin_can_list_sources() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    for i in 0..3 {
        let body = format!(
            r#"{{"name":"src-{i}","source_type":"base64","url":"https://example.com/{i}"}}"#
        );
        let response = router
            .clone()
            .oneshot(with_cookie(post_json("/api/v1/sources", &body), &cookie))
            .await
            .expect("create");
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let response = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/sources?limit=2"), &cookie))
        .await
        .expect("list");

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert!(json["sources"].as_array().is_some_and(|a| a.len() == 2));
    assert!(json["next_cursor"].as_str().is_some());
}

/// SRC-001: Admin can get a source by ID.
#[tokio::test]
async fn admin_can_get_source_by_id() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", VALID_SOURCE_BODY),
            &cookie,
        ))
        .await
        .expect("create");
    let json = body_to_json(response).await;
    let id = json["source"]["id"].as_str().expect("id");

    let response = router
        .clone()
        .oneshot(with_cookie(get(&format!("/api/v1/sources/{id}")), &cookie))
        .await
        .expect("get");

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert_eq!(json["source"]["name"], "my-sub");
}

/// SRC-001: Getting a non-existent source returns 404.
#[tokio::test]
async fn get_nonexistent_returns_404() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    // WHY: 01J0 is the reserved test-identifier prefix per AGENTS.md; this
    // ULID cannot collide with a real one generated by SourceId::new().
    let fake_id = "01J0ZZZZZZZZZZZZZZZZZZZZZX";
    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/sources/{fake_id}")),
            &cookie,
        ))
        .await
        .expect("get");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// SRC-001: Invalid ULID returns 400.
#[tokio::test]
async fn invalid_id_returns_400() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/sources/not-a-ulid"), &cookie))
        .await
        .expect("get");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// SRC-001: Admin can update a source.
#[tokio::test]
async fn admin_can_update_source() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", VALID_SOURCE_BODY),
            &cookie,
        ))
        .await
        .expect("create");
    let json = body_to_json(response).await;
    let id = json["source"]["id"].as_str().expect("id");

    let update_body = r#"{"name":"updated-sub","source_type":"uri_list","url":"https://example.com/new","auto_update":false,"update_interval_secs":7200,"enabled":false,"keep_on_fail":false}"#;
    let response = router
        .clone()
        .oneshot(with_cookie(
            put_json(&format!("/api/v1/sources/{id}"), update_body),
            &cookie,
        ))
        .await
        .expect("update");

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert_eq!(json["source"]["name"], "updated-sub");
    assert_eq!(json["source"]["source_type"], "uri_list");
    assert_eq!(json["source"]["url"], "https://example.com/new");
    assert_eq!(json["source"]["auto_update"], false);
    assert_eq!(json["source"]["update_interval_secs"], 7200);
    assert_eq!(json["source"]["enabled"], false);
    assert_eq!(json["source"]["keep_on_fail"], false);
}

/// SRC-001: Updating a non-existent source returns 404.
#[tokio::test]
async fn update_nonexistent_returns_404() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let fake_id = "01J0ZZZZZZZZZZZZZZZZZZZZZX";
    let response = router
        .clone()
        .oneshot(with_cookie(
            put_json(
                &format!("/api/v1/sources/{fake_id}"),
                r#"{"name":"x","source_type":"auto","url":"https://example.com/x","auto_update":false,"update_interval_secs":3600,"enabled":true,"keep_on_fail":true}"#,
            ),
            &cookie,
        ))
        .await
        .expect("update");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// SRC-001: Admin can delete a source.
#[tokio::test]
async fn admin_can_delete_source() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", VALID_SOURCE_BODY),
            &cookie,
        ))
        .await
        .expect("create");
    let json = body_to_json(response).await;
    let id = json["source"]["id"].as_str().expect("id");

    let response = router
        .clone()
        .oneshot(with_cookie(
            delete(&format!("/api/v1/sources/{id}")),
            &cookie,
        ))
        .await
        .expect("delete");
    assert_eq!(response.status(), StatusCode::OK);

    let response = router
        .clone()
        .oneshot(with_cookie(get(&format!("/api/v1/sources/{id}")), &cookie))
        .await
        .expect("get after delete");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// SRC-001: Deleting a non-existent source returns 404.
#[tokio::test]
async fn delete_nonexistent_returns_404() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let fake_id = "01J0ZZZZZZZZZZZZZZZZZZZZZX";
    let response = router
        .clone()
        .oneshot(with_cookie(
            delete(&format!("/api/v1/sources/{fake_id}")),
            &cookie,
        ))
        .await
        .expect("delete");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// SRC-001: Defaults are applied when optional fields are omitted.
#[tokio::test]
async fn defaults_applied() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/sources",
                r#"{"name":"minimal","source_type":"auto","url":"https://example.com/min"}"#,
            ),
            &cookie,
        ))
        .await
        .expect("create");

    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    assert_eq!(json["source"]["auto_update"], false);
    assert_eq!(json["source"]["update_interval_secs"], 3600);
    assert_eq!(json["source"]["keep_on_fail"], true);
}

/// SRC-001: Regular (non-admin) users get 403 on source routes.
#[tokio::test]
async fn regular_user_forbidden_on_source_routes() {
    let app = TestApp::new().await;
    let router = app.router();
    let admin_cookie = setup_and_login(&router).await;

    let create_body = r#"{"username":"bob","password":"user-pwd!","role":"user"}"#;
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/users", create_body),
            &admin_cookie,
        ))
        .await
        .expect("create user");
    assert_eq!(response.status(), StatusCode::CREATED);

    let login_body = r#"{"username":"bob","password":"user-pwd!"}"#;
    let response = router
        .clone()
        .oneshot(post_json("/api/v1/auth/login", login_body))
        .await
        .expect("login");
    assert_eq!(response.status(), StatusCode::OK);
    let user_cookie = extract_cookie(&response).expect("cookie");

    let response = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/sources"), &user_cookie))
        .await
        .expect("list");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", VALID_SOURCE_BODY),
            &user_cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// SRC-001: Updating a source name to an existing name returns 409.
#[tokio::test]
async fn update_to_duplicate_name_returns_conflict() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let _ = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", VALID_SOURCE_BODY),
            &cookie,
        ))
        .await
        .expect("first source");

    let second_body =
        r#"{"name":"second","source_type":"base64","url":"https://example.com/other"}"#;
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/sources", second_body),
            &cookie,
        ))
        .await
        .expect("second source");
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    let second_id = json["source"]["id"].as_str().expect("id").to_owned();

    // WHY: rename "second" to "my-sub" (already taken by the first source).
    let update_body = r#"{"name":"my-sub","source_type":"base64","url":"https://example.com/other","auto_update":false,"update_interval_secs":3600,"enabled":true,"keep_on_fail":true}"#;
    let response = router
        .clone()
        .oneshot(with_cookie(
            put_json(&format!("/api/v1/sources/{second_id}"), update_body),
            &cookie,
        ))
        .await
        .expect("update");
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

/// SRC-001: Cursor pagination returns the correct second page.
#[tokio::test]
async fn pagination_second_page() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    for i in 0..3 {
        let body = format!(
            r#"{{"name":"page-{i}","source_type":"base64","url":"https://example.com/{i}"}}"#
        );
        let _ = router
            .clone()
            .oneshot(with_cookie(post_json("/api/v1/sources", &body), &cookie))
            .await
            .expect("create");
    }

    let response = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/sources?limit=2"), &cookie))
        .await
        .expect("page 1");
    let json = body_to_json(response).await;
    let page1 = json["sources"].as_array().expect("sources array");
    assert_eq!(page1.len(), 2);
    let cursor = json["next_cursor"]
        .as_str()
        .expect("next_cursor")
        .to_owned();

    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/sources?limit=2&cursor={cursor}")),
            &cookie,
        ))
        .await
        .expect("page 2");
    let json = body_to_json(response).await;
    let page2 = json["sources"].as_array().expect("sources array");
    assert_eq!(page2.len(), 1, "second page should have 1 remaining source");
    assert_eq!(page2[0]["name"], "page-2");
}

/// Helper trait to add a header to an existing Request<Body>.
trait RequestExt {
    fn with_header(self, key: &str, value: String) -> Self;
}

impl RequestExt for Request<Body> {
    fn with_header(mut self, key: &str, value: String) -> Self {
        let name = axum::http::HeaderName::from_str(key).expect("header name");
        self.headers_mut()
            .insert(name, value.parse().expect("header"));
        self
    }
}
