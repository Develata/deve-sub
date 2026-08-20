#![allow(clippy::expect_used)]

//! Integration tests for `POST /api/v1/sources/{id}/refresh` (SRC-002).
//!
//! Covers admin auth, 404 on non-existent source, 400 on invalid ULID, 502 on
//! fetch failure, and successful refresh returning reconcile counts.

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use deve_sub_application::{
    DbHealthPort, FetchError, FetchResult, GeoIpPort, LoginRateLimiter, SubscriptionFetcher,
};
use deve_sub_domain::{
    AuditLogRepository, GenerationCacheRepository, LatencyProbe, LatencyRecordRepository,
    NodeOverrideRepository, NodePoolRepository, PoolMetaRepository, ProbeRunRepository,
    ProbeSourceRepository, RecoveryCodeRepository, SessionRepository, ShortCodeRepository,
    SourceRefreshJobRepository, SourceRepository, SourceSnapshotRepository, SubscriptionRepository,
    SubscriptionTokenRepository, TempLinkRepository, TemplateRepository, TemplateVersionRepository,
    TotpSecretRepository, TrafficDailySnapshotRepository, TrafficRepository, UserRepository,
};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{
    SqliteAuditLogRepository, SqliteGenerationCacheRepository, SqliteHealthCheck,
    SqliteLatencyRecordRepository, SqliteNodeOverrideRepository, SqliteNodePoolRepository,
    SqlitePoolMetaRepository, SqliteProbeRunRepository, SqliteProbeSourceRepository,
    SqliteRecoveryCodeRepository, SqliteSessionRepository, SqliteShortCodeRepository,
    SqliteSourceRefreshJobRepository, SqliteSourceRepository, SqliteSourceSnapshotRepository,
    SqliteSubscriptionRepository, SqliteSubscriptionTokenRepository, SqliteTempLinkRepository,
    SqliteTemplateRepository, SqliteTemplateVersionRepository, SqliteTotpSecretRepository,
    SqliteTrafficDailySnapshotRepository, SqliteTrafficRepository, SqliteUserRepository,
};

struct MockFetcher {
    response: Mutex<Option<MockResp>>,
    delay_ms: u64,
}

enum MockResp {
    Ok {
        body: Vec<u8>,
        etag: Option<String>,
        content_type: Option<String>,
    },
    Error(FetchError),
}

impl MockFetcher {
    fn ok(body: &str) -> Self {
        Self {
            response: Mutex::new(Some(MockResp::Ok {
                body: body.as_bytes().to_vec(),
                etag: None,
                content_type: Some("text/plain".to_owned()),
            })),
            delay_ms: 0,
        }
    }
    fn error() -> Self {
        Self {
            response: Mutex::new(Some(MockResp::Error(FetchError::Timeout(30)))),
            delay_ms: 0,
        }
    }
    fn slow(body: &str, delay_ms: u64) -> Self {
        Self {
            response: Mutex::new(Some(MockResp::Ok {
                body: body.as_bytes().to_vec(),
                etag: None,
                content_type: Some("text/plain".to_owned()),
            })),
            delay_ms,
        }
    }
}

#[async_trait]
impl SubscriptionFetcher for MockFetcher {
    async fn fetch(&self, _url: &str, _etag: Option<&str>) -> Result<FetchResult, FetchError> {
        if self.delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
        }
        let resp = self.response.lock().expect("mutex").take();
        match resp {
            Some(MockResp::Ok {
                body,
                etag,
                content_type,
            }) => Ok(FetchResult::Ok {
                body,
                etag,
                content_type,
            }),
            Some(MockResp::Error(e)) => Err(e),
            None => Err(FetchError::Connection("no mock response".to_owned())),
        }
    }
}

struct TestApp {
    state: deve_sub_server::AppState,
    _dir: tempfile::TempDir,
}

impl TestApp {
    async fn new_with_fetcher(fetcher: MockFetcher) -> Self {
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
                source_repo: Arc::new(SqliteSourceRepository::new_with_key(
                    pool.clone(),
                    Arc::clone(&master_key),
                )) as Arc<dyn SourceRepository>,
                snapshot_repo: Arc::new(SqliteSourceSnapshotRepository::new(pool.clone()))
                    as Arc<dyn SourceSnapshotRepository>,
                refresh_job_repo: Arc::new(SqliteSourceRefreshJobRepository::new(pool.clone()))
                    as Arc<dyn SourceRefreshJobRepository>,
                pool_repo: Arc::new(SqliteNodePoolRepository::new_with_key(
                    pool.clone(),
                    Arc::clone(&master_key),
                )) as Arc<dyn NodePoolRepository>,
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
                probe_source_repo: Arc::new(SqliteProbeSourceRepository::new_with_key(
                    pool.clone(),
                    Arc::clone(&master_key),
                )) as Arc<dyn ProbeSourceRepository>,
                probe_run_repo: Arc::new(SqliteProbeRunRepository::new(pool.clone()))
                    as Arc<dyn ProbeRunRepository>,
                latency_repo: Arc::new(SqliteLatencyRecordRepository::new(pool.clone()))
                    as Arc<dyn LatencyRecordRepository>,
                probe_adapter: std::sync::Arc::new(
                    deve_sub_adapters::ProbeSourceAdapterRegistry::new()
                        .with_nezha(std::sync::Arc::new(
                            deve_sub_adapters::NezhaProbeAdapter::new(),
                        ))
                        .with_dstatus(std::sync::Arc::new(
                            deve_sub_adapters::DStatusProbeAdapter::new(),
                        ))
                        .with_komari(std::sync::Arc::new(
                            deve_sub_adapters::KomariProbeAdapter::new(),
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
                refresh_cancel_flags: Arc::new(std::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
                job_supervisor: Arc::new(deve_sub_application::JobSupervisor::new()),
                geoip: Arc::new(deve_sub_inmemory::InMemoryGeoIp::new()) as Arc<dyn GeoIpPort>,
                fetcher: Arc::new(fetcher) as Arc<dyn SubscriptionFetcher>,
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

fn post(uri: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .body(Body::empty())
        .expect("request")
}

fn post_json(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .expect("request")
}

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

async fn create_source(router: &axum::Router, cookie: &str) -> String {
    let body = r#"{"name":"my-sub","source_type":"uri_list","url":"https://example.com/sub","auto_update":false,"update_interval_secs":3600,"keep_on_fail":true}"#;
    let response = router
        .clone()
        .oneshot(with_cookie(post_json("/api/v1/sources", body), cookie))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    json["source"]["id"].as_str().expect("id").to_owned()
}

async fn body_to_json(response: axum::response::Response) -> serde_json::Value {
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    serde_json::from_slice(&body).expect("json")
}

const TROJAN_LIST: &str = "trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#NodeA";

/// Poll `GET /api/v1/sources/refresh-jobs/{job_id}` until the job reaches a
/// terminal status or the iteration budget is exhausted.
async fn poll_job(router: &axum::Router, cookie: &str, job_id: &str) -> serde_json::Value {
    for _ in 0..50 {
        let response = router
            .clone()
            .oneshot(with_cookie(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/sources/refresh-jobs/{job_id}"))
                    .body(Body::empty())
                    .expect("request"),
                cookie,
            ))
            .await
            .expect("poll");
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_to_json(response).await;
        let status = json["status"].as_str().expect("status");
        if status == "completed" || status == "failed" || status == "cancelled" {
            return json;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("refresh job did not reach terminal status within 5s");
}

/// SRC-002: Admin can refresh a source; the job completes with reconcile counts.
#[tokio::test]
async fn admin_can_refresh_source() {
    let app = TestApp::new_with_fetcher(MockFetcher::ok(TROJAN_LIST)).await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let source_id = create_source(&router, &cookie).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post(&format!("/api/v1/sources/{source_id}/refresh")),
            &cookie,
        ))
        .await
        .expect("refresh");

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let json = body_to_json(response).await;
    assert_eq!(json["status"], "running");
    let job_id = json["job_id"].as_str().expect("job_id").to_owned();

    let job = poll_job(&router, &cookie, &job_id).await;
    assert_eq!(job["status"], "completed");
    assert_eq!(job["new_nodes"], 1);
    assert_eq!(job["duplicate_nodes"], 0);
    assert_eq!(job["not_modified"], false);
}

/// SRC-002: Unauthenticated refresh returns 401.
#[tokio::test]
async fn unauthenticated_refresh_rejected() {
    let app = TestApp::new_with_fetcher(MockFetcher::ok(TROJAN_LIST)).await;
    let router = app.router();

    let response = router
        .clone()
        .oneshot(post("/api/v1/sources/01J0ZZZZZZZZZZZZZZZZZZZZZX/refresh"))
        .await
        .expect("refresh");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// SRC-002: Non-admin user gets 403.
#[tokio::test]
async fn non_admin_forbidden() {
    let app = TestApp::new_with_fetcher(MockFetcher::ok(TROJAN_LIST)).await;
    let router = app.router();
    let admin_cookie = setup_and_login(&router).await;

    let create_user = r#"{"username":"bob","password":"user-pwd!","role":"user"}"#;
    let _ = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/users", create_user),
            &admin_cookie,
        ))
        .await
        .expect("create user");

    let login = r#"{"username":"bob","password":"user-pwd!"}"#;
    let response = router
        .clone()
        .oneshot(post_json("/api/v1/auth/login", login))
        .await
        .expect("login user");
    let user_cookie = extract_cookie(&response).expect("cookie");

    let response = router
        .clone()
        .oneshot(with_cookie(
            post("/api/v1/sources/01J0ZZZZZZZZZZZZZZZZZZZZZX/refresh"),
            &user_cookie,
        ))
        .await
        .expect("refresh");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// SRC-002: Refreshing a non-existent source returns 404 immediately.
#[tokio::test]
async fn refresh_nonexistent_returns_404() {
    let app = TestApp::new_with_fetcher(MockFetcher::ok(TROJAN_LIST)).await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post("/api/v1/sources/01J0ZZZZZZZZZZZZZZZZZZZZZX/refresh"),
            &cookie,
        ))
        .await
        .expect("refresh");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// SRC-002: Invalid ULID returns 400.
#[tokio::test]
async fn refresh_invalid_id_returns_400() {
    let app = TestApp::new_with_fetcher(MockFetcher::ok(TROJAN_LIST)).await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post("/api/v1/sources/not-a-ulid/refresh"),
            &cookie,
        ))
        .await
        .expect("refresh");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// SRC-002: Fetch failure produces a `failed` job (not 502 synchronously).
#[tokio::test]
async fn refresh_fetch_failure_marks_job_failed() {
    let app = TestApp::new_with_fetcher(MockFetcher::error()).await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let source_id = create_source(&router, &cookie).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post(&format!("/api/v1/sources/{source_id}/refresh")),
            &cookie,
        ))
        .await
        .expect("refresh");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let json = body_to_json(response).await;
    let job_id = json["job_id"].as_str().expect("job_id").to_owned();

    let job = poll_job(&router, &cookie, &job_id).await;
    assert_eq!(job["status"], "failed");
    assert!(job["error_message"].as_str().is_some_and(|s| !s.is_empty()));
}

/// SRC-009: A second refresh while one is running returns 409.
#[tokio::test]
async fn concurrent_refresh_returns_409() {
    let app = TestApp::new_with_fetcher(MockFetcher::slow(TROJAN_LIST, 500)).await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let source_id = create_source(&router, &cookie).await;

    let first = router
        .clone()
        .oneshot(with_cookie(
            post(&format!("/api/v1/sources/{source_id}/refresh")),
            &cookie,
        ))
        .await
        .expect("first refresh");
    assert_eq!(first.status(), StatusCode::ACCEPTED);

    let second = router
        .clone()
        .oneshot(with_cookie(
            post(&format!("/api/v1/sources/{source_id}/refresh")),
            &cookie,
        ))
        .await
        .expect("second refresh");
    assert_eq!(second.status(), StatusCode::CONFLICT);
}
