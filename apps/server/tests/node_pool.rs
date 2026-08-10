#![allow(clippy::expect_used)]

//! Integration tests for node pool endpoints (NODE-001/002/003/011).
//!
//! Covers `GET /api/v1/nodes`, `GET /api/v1/nodes/{id}`, and
//! `POST /api/v1/nodes/import`. All routes are admin-only. See
//! `docs/plan/milestones/M4-sources-and-node-pool.md` Slice 3.

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
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .expect("body");
    serde_json::from_slice(&body).expect("json")
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

const IMPORT_BODY: &str = "{\"content\":\"trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#NodeA\\ntrojan://TEST_PASSWORD@other.com:8443?sni=other.com&type=tcp#NodeB\",\"source_type\":\"uri_list\"}";

/// NODE-001: Admin can import a batch of nodes via POST /api/v1/nodes/import.
#[tokio::test]
async fn admin_can_import_nodes() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/nodes/import", IMPORT_BODY),
            &cookie,
        ))
        .await
        .expect("import");

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert_eq!(json["new_nodes"], 2);
    assert_eq!(json["duplicate_nodes"], 0);
    assert_eq!(json["failed"], 0);
    assert_eq!(
        json["outcomes"].as_array().expect("outcomes array").len(),
        2
    );
}

/// NODE-003: Importing a duplicate node does not overwrite the existing one.
#[tokio::test]
async fn import_duplicate_is_counted_not_overwritten() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    // First import.
    let r1 = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/nodes/import", IMPORT_BODY),
            &cookie,
        ))
        .await
        .expect("first import");
    assert_eq!(r1.status(), StatusCode::OK);
    let j1 = body_to_json(r1).await;
    assert_eq!(j1["new_nodes"], 2);

    // Second import of the same content → all duplicates.
    let r2 = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/nodes/import", IMPORT_BODY),
            &cookie,
        ))
        .await
        .expect("second import");
    assert_eq!(r2.status(), StatusCode::OK);
    let j2 = body_to_json(r2).await;
    assert_eq!(j2["new_nodes"], 0);
    assert_eq!(j2["duplicate_nodes"], 2);
}

/// GET /api/v1/nodes returns the imported nodes.
#[tokio::test]
async fn list_returns_imported_nodes() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let _ = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/nodes/import", IMPORT_BODY),
            &cookie,
        ))
        .await
        .expect("import");

    let response = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/nodes"), &cookie))
        .await
        .expect("list");

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    let nodes = json["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().all(|n| n["is_active"] == true));
    assert!(nodes.iter().all(|n| n["missing_from_source"] == false));
}

/// GET /api/v1/nodes/{id} returns a single node.
#[tokio::test]
async fn get_node_by_id() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let _ = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/nodes/import", IMPORT_BODY),
            &cookie,
        ))
        .await
        .expect("import");

    let list_response = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/nodes"), &cookie))
        .await
        .expect("list");
    let list_json = body_to_json(list_response).await;
    let node_id = list_json["nodes"][0]["id"]
        .as_str()
        .expect("node id")
        .to_owned();

    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/nodes/{node_id}")),
            &cookie,
        ))
        .await
        .expect("get");

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert_eq!(json["node"]["id"], node_id);
    assert_eq!(json["node"]["protocol"], "Trojan");
}

/// GET /api/v1/nodes/{id} with unknown ID returns 404.
#[tokio::test]
async fn get_node_unknown_returns_404() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let fake_id = deve_sub_kernel::NodeId::new().to_string();
    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/nodes/{fake_id}")),
            &cookie,
        ))
        .await
        .expect("get");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// Unauthenticated requests to node endpoints are rejected with 401.
#[tokio::test]
async fn unauthenticated_rejected() {
    let app = TestApp::new().await;
    let router = app.router();

    let list_r = router
        .clone()
        .oneshot(get("/api/v1/nodes"))
        .await
        .expect("list");
    assert_eq!(list_r.status(), StatusCode::UNAUTHORIZED);

    let import_r = router
        .clone()
        .oneshot(post_json("/api/v1/nodes/import", IMPORT_BODY))
        .await
        .expect("import");
    assert_eq!(import_r.status(), StatusCode::UNAUTHORIZED);
}

/// Import with empty content returns 400.
#[tokio::test]
async fn import_empty_content_returns_400() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/nodes/import",
                r#"{"content":"","source_type":"uri_list"}"#,
            ),
            &cookie,
        ))
        .await
        .expect("import");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Import with unparseable content returns 400.
#[tokio::test]
async fn import_unparseable_returns_400() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    // Mihomo YAML with invalid structure.
    let body = r#"{"content":"not: valid: yaml: ???","source_type":"mihomo_yaml"}"#;
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/nodes/import", body),
            &cookie,
        ))
        .await
        .expect("import");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
