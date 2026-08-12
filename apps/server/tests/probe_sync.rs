#![allow(clippy::expect_used)]

//! E2E tests for M7 Slice 4: Nezha probe traffic sync (PROBE-001).
//!
//! Spins up a mock Nezha panel HTTP server, creates a probe source bound to
//! a subscription, calls `POST /api/v1/probe-sources/{id}/sync`, and verifies
//! that traffic records are written and the counter snapshot is persisted.
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` §"Probe source
//! adapter Port" and `docs/acceptance/matrix.tsv` PROBE-001.

use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tokio::net::TcpListener;
use tower::ServiceExt;

use deve_sub_application::{DbHealthPort, GeoIpPort, LoginRateLimiter, SubscriptionFetcher};
use deve_sub_domain::{
    AuditLogRepository, GenerationCacheRepository, LatencyProbe, LatencyRecordRepository,
    NodeOverrideRepository, NodePoolRepository, PoolMetaRepository, ProbeRunRepository,
    ProbeSourceAdapter, ProbeSourceRepository, RecoveryCodeRepository, SessionRepository,
    ShortCodeRepository, SourceRepository, SourceSnapshotRepository, SubscriptionRepository,
    SubscriptionTokenRepository, TempLinkRepository, TemplateRepository, TemplateVersionRepository,
    TotpSecretRepository, TrafficRepository, UserRepository,
};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{
    SqliteAuditLogRepository, SqliteGenerationCacheRepository, SqliteHealthCheck,
    SqliteLatencyRecordRepository, SqliteNodeOverrideRepository, SqliteNodePoolRepository,
    SqlitePoolMetaRepository, SqliteProbeRunRepository, SqliteProbeSourceRepository,
    SqliteRecoveryCodeRepository, SqliteSessionRepository, SqliteShortCodeRepository,
    SqliteSourceRepository, SqliteSourceSnapshotRepository, SqliteSubscriptionRepository,
    SqliteSubscriptionTokenRepository, SqliteTempLinkRepository, SqliteTemplateRepository,
    SqliteTemplateVersionRepository, SqliteTotpSecretRepository, SqliteTrafficRepository,
    SqliteUserRepository,
};

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

/// Mock Nezha panel: returns cumulative counter JSON for `GET /api/v1/server`.
/// Each call increments counters so the second sync produces a delta.
struct MockNezha {
    net_in: Arc<AtomicU64>,
    net_out: Arc<AtomicU64>,
}

impl MockNezha {
    fn new() -> Self {
        Self {
            net_in: Arc::new(AtomicU64::new(10_000)),
            net_out: Arc::new(AtomicU64::new(20_000)),
        }
    }

    async fn start(&self) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let bind = format!("http://{addr}");

        let net_in = self.net_in.clone();
        let net_out = self.net_out.clone();

        tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                let net_in = net_in.clone();
                let net_out = net_out.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
                    let cur_in = net_in.load(Ordering::Relaxed);
                    let cur_out = net_out.load(Ordering::Relaxed);
                    let body = format!(
                        r#"[{{"id":1,"uuid":"srv-1","state":{{"net_in_transfer":{cur_in},"net_out_transfer":{cur_out}}}}},{{"id":2,"uuid":"srv-2","state":{{"net_in_transfer":5000,"net_out_transfer":6000}}}}]"#
                    );
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    use tokio::io::AsyncWriteExt;
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        bind
    }

    fn bump(&self) {
        self.net_in.fetch_add(3000, Ordering::Relaxed);
        self.net_out.fetch_add(5000, Ordering::Relaxed);
    }
}

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

        let master_key = Arc::new(
            MasterKey::load_or_generate(std::path::Path::new(&key_path)).expect("master key"),
        );

        let config = deve_sub_application::AppConfig::default();

        let rate_limiter: Arc<dyn LoginRateLimiter> =
            Arc::new(deve_sub_inmemory::InMemoryLoginRateLimiter::new(
                config.security.max_login_attempts,
                std::time::Duration::from_secs(config.security.lockout_duration_secs),
            ));

        let db_health: Arc<dyn DbHealthPort> = Arc::new(SqliteHealthCheck::new(pool.clone()));

        let nezha: Arc<dyn ProbeSourceAdapter> =
            Arc::new(deve_sub_adapters::NezhaProbeAdapter::with_checker(
                Arc::clone(&master_key),
                Arc::new(deve_sub_adapters::PermissiveSsrfChecker),
            ));
        let dstatus: Arc<dyn ProbeSourceAdapter> =
            Arc::new(deve_sub_adapters::DStatusProbeAdapter::with_checker(
                Arc::clone(&master_key),
                Arc::new(deve_sub_adapters::PermissiveSsrfChecker),
            ));
        let komari: Arc<dyn ProbeSourceAdapter> =
            Arc::new(deve_sub_adapters::KomariProbeAdapter::with_checker(
                Arc::clone(&master_key),
                Arc::new(deve_sub_adapters::PermissiveSsrfChecker),
            ));
        let probe_adapter: Arc<dyn ProbeSourceAdapter> = Arc::new(
            deve_sub_adapters::ProbeSourceAdapterRegistry::new()
                .with_nezha(nezha)
                .with_dstatus(dstatus)
                .with_komari(komari),
        );

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
                probe_source_repo: Arc::new(SqliteProbeSourceRepository::new(pool.clone()))
                    as Arc<dyn ProbeSourceRepository>,
                probe_run_repo: Arc::new(SqliteProbeRunRepository::new(pool.clone()))
                    as Arc<dyn ProbeRunRepository>,
                latency_repo: Arc::new(SqliteLatencyRecordRepository::new(pool.clone()))
                    as Arc<dyn LatencyRecordRepository>,
                probe_adapter,
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

async fn body_to_json(response: axum::response::Response) -> serde_json::Value {
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .expect("body");
    serde_json::from_slice(&body).expect("json")
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

async fn create_subscription(router: &axum::Router, cookie: &str, template_id: &str) -> String {
    let body = serde_json::json!({
        "name": "probe-sub",
        "slug": "probe-sub",
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
    let v = body_to_json(res).await;
    v["subscription"]["id"]
        .as_str()
        .expect("subscription id")
        .to_owned()
}

/// PROBE-001: Nezha traffic sync writes TrafficRecord rows and persists the
/// encrypted counter snapshot. A second sync computes the delta.
#[tokio::test]
async fn probe001_nezha_sync_writes_traffic_and_snapshot() {
    let mock = MockNezha::new();
    let endpoint = mock.start().await;

    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;
    let subscription_id = create_subscription(&router, &cookie, &template_id).await;

    let create_body = serde_json::json!({
        "kind": "nezha",
        "name": "my-nezha",
        "endpoint_url": endpoint,
        "auth_config": "nzp_test_token",
        "subscription_id": subscription_id,
    })
    .to_string();
    let resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-sources", &create_body),
            &cookie,
        ))
        .await
        .expect("create source");
    assert_eq!(resp.status(), StatusCode::CREATED);
    let source_json = body_to_json(resp).await;
    let source_id = source_json["source"]["id"]
        .as_str()
        .expect("source id")
        .to_owned();

    let sync_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json(&format!("/api/v1/probe-sources/{source_id}/sync"), ""),
            &cookie,
        ))
        .await
        .expect("sync");
    assert_eq!(sync_resp.status(), StatusCode::OK);
    let sync_json = body_to_json(sync_resp).await;
    assert_eq!(sync_json["samples_written"], 2);
    assert_eq!(sync_json["snapshot_updated"], true);
    assert!(
        sync_json["source"]["last_sync_at"].as_str().is_some(),
        "last_sync_at should be set after sync"
    );

    let traffic_resp = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/subscriptions/{subscription_id}/traffic"))
                .body(Body::empty())
                .expect("request"),
            &cookie,
        ))
        .await
        .expect("traffic");
    assert_eq!(traffic_resp.status(), StatusCode::OK);
    let traffic_json = body_to_json(traffic_resp).await;
    let total_upload = traffic_json["upload"].as_u64().expect("upload");
    let total_download = traffic_json["download"].as_u64().expect("download");
    assert!(
        total_upload >= 15_000,
        "first sync upload should include both servers' counters: {total_upload}"
    );
    assert!(
        total_download >= 26_000,
        "first sync download should include both servers' counters: {total_download}"
    );

    mock.bump();

    let sync2_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json(&format!("/api/v1/probe-sources/{source_id}/sync"), ""),
            &cookie,
        ))
        .await
        .expect("sync2");
    assert_eq!(sync2_resp.status(), StatusCode::OK);
    let sync2_json = body_to_json(sync2_resp).await;
    assert_eq!(sync2_json["samples_written"], 1);
    assert_eq!(sync2_json["snapshot_updated"], true);

    let traffic2_resp = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/subscriptions/{subscription_id}/traffic"))
                .body(Body::empty())
                .expect("request"),
            &cookie,
        ))
        .await
        .expect("traffic2");
    let traffic2_json = body_to_json(traffic2_resp).await;
    let upload2 = traffic2_json["upload"].as_u64().expect("upload2");
    let download2 = traffic2_json["download"].as_u64().expect("download2");
    assert_eq!(
        upload2,
        total_upload + 3000,
        "second sync delta should add exactly 3000 upload bytes"
    );
    assert_eq!(
        download2,
        total_download + 5000,
        "second sync delta should add exactly 5000 download bytes"
    );
}

/// PROBE-001 (negative): sync without subscription binding returns 400.
#[tokio::test]
async fn probe001_sync_without_subscription_returns_400() {
    let mock = MockNezha::new();
    let endpoint = mock.start().await;

    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let create_body = serde_json::json!({
        "kind": "nezha",
        "name": "no-sub-nezha",
        "endpoint_url": endpoint,
        "auth_config": "nzp_test_token",
    })
    .to_string();
    let resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-sources", &create_body),
            &cookie,
        ))
        .await
        .expect("create source");
    assert_eq!(resp.status(), StatusCode::CREATED);
    let source_json = body_to_json(resp).await;
    let source_id = source_json["source"]["id"]
        .as_str()
        .expect("source id")
        .to_owned();

    let sync_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json(&format!("/api/v1/probe-sources/{source_id}/sync"), ""),
            &cookie,
        ))
        .await
        .expect("sync");
    assert_eq!(sync_resp.status(), StatusCode::BAD_REQUEST);
    let sync_json = body_to_json(sync_resp).await;
    assert_eq!(sync_json["error"], "invalid_input");
}

/// PROBE-001 (negative): sync a disabled source returns 400.
#[tokio::test]
async fn probe001_sync_disabled_source_returns_400() {
    let mock = MockNezha::new();
    let endpoint = mock.start().await;

    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;
    let subscription_id = create_subscription(&router, &cookie, &template_id).await;

    let create_body = serde_json::json!({
        "kind": "nezha",
        "name": "disabled-nezha",
        "endpoint_url": endpoint,
        "auth_config": "nzp_test_token",
        "subscription_id": subscription_id,
    })
    .to_string();
    let resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-sources", &create_body),
            &cookie,
        ))
        .await
        .expect("create source");
    assert_eq!(resp.status(), StatusCode::CREATED);
    let source_json = body_to_json(resp).await;
    let source_id = source_json["source"]["id"]
        .as_str()
        .expect("source id")
        .to_owned();

    let disable_body = serde_json::json!({"enabled": false}).to_string();
    let _ = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/probe-sources/{source_id}"))
                .header("content-type", "application/json")
                .body(json_body(&disable_body))
                .expect("request"),
            &cookie,
        ))
        .await
        .expect("disable");

    let sync_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json(&format!("/api/v1/probe-sources/{source_id}/sync"), ""),
            &cookie,
        ))
        .await
        .expect("sync");
    assert_eq!(sync_resp.status(), StatusCode::BAD_REQUEST);
    let sync_json = body_to_json(sync_resp).await;
    assert_eq!(sync_json["error"], "invalid_input");
}

// ---------------------------------------------------------------------------
// PROBE-002: DStatus traffic sync
// ---------------------------------------------------------------------------

/// Mock DStatus panel: returns JSON for `GET /api/allnode_status`.
struct MockDStatus {
    used_a: Arc<AtomicU64>,
    used_b: Arc<AtomicU64>,
}

impl MockDStatus {
    fn new() -> Self {
        Self {
            used_a: Arc::new(AtomicU64::new(10_000_000_000)),
            used_b: Arc::new(AtomicU64::new(5_000_000_000)),
        }
    }

    async fn start(&self) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let bind = format!("http://{addr}");

        let used_a = self.used_a.clone();
        let used_b = self.used_b.clone();

        tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                let used_a = used_a.clone();
                let used_b = used_b.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
                    let ua = used_a.load(Ordering::Relaxed);
                    let ub = used_b.load(Ordering::Relaxed);
                    let body = format!(
                        r#"{{"success":true,"order":["node-a","node-b"],"data":{{"node-a":{{"name":"Server A","status":1,"traffic_stats":{{"used":{ua},"limit":0,"unlimited":true}}}},"node-b":{{"name":"Server B","status":1,"traffic_stats":{{"used":{ub},"limit":0,"unlimited":true}}}}}}}}"#
                    );
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    use tokio::io::AsyncWriteExt;
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        bind
    }

    fn bump(&self) {
        self.used_a.fetch_add(2_000_000_000, Ordering::Relaxed);
        self.used_b.fetch_add(1_000_000_000, Ordering::Relaxed);
    }
}

/// PROBE-002: DStatus traffic sync writes TrafficRecord rows and persists
/// the encrypted counter snapshot. A second sync computes the delta.
#[tokio::test]
async fn probe002_dstatus_sync_writes_traffic_and_snapshot() {
    let mock = MockDStatus::new();
    let endpoint = mock.start().await;

    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;
    let subscription_id = create_subscription(&router, &cookie, &template_id).await;

    let create_body = serde_json::json!({
        "kind": "dstatus",
        "name": "my-dstatus",
        "endpoint_url": endpoint,
        "auth_config": "",
        "subscription_id": subscription_id,
    })
    .to_string();
    let resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-sources", &create_body),
            &cookie,
        ))
        .await
        .expect("create source");
    assert_eq!(resp.status(), StatusCode::CREATED);
    let source_json = body_to_json(resp).await;
    let source_id = source_json["source"]["id"]
        .as_str()
        .expect("source id")
        .to_owned();

    let sync_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json(&format!("/api/v1/probe-sources/{source_id}/sync"), ""),
            &cookie,
        ))
        .await
        .expect("sync");
    assert_eq!(sync_resp.status(), StatusCode::OK);
    let sync_json = body_to_json(sync_resp).await;
    assert_eq!(sync_json["samples_written"], 2);
    assert_eq!(sync_json["snapshot_updated"], true);

    let traffic_resp = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/subscriptions/{subscription_id}/traffic"))
                .body(Body::empty())
                .expect("request"),
            &cookie,
        ))
        .await
        .expect("traffic");
    assert_eq!(traffic_resp.status(), StatusCode::OK);
    let traffic_json = body_to_json(traffic_resp).await;
    let total_download = traffic_json["download"].as_u64().expect("download");
    assert!(
        total_download >= 15_000_000_000,
        "first sync download should include both nodes: {total_download}"
    );

    mock.bump();

    let sync2_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json(&format!("/api/v1/probe-sources/{source_id}/sync"), ""),
            &cookie,
        ))
        .await
        .expect("sync2");
    assert_eq!(sync2_resp.status(), StatusCode::OK);
    let sync2_json = body_to_json(sync2_resp).await;
    assert_eq!(sync2_json["samples_written"], 2);
    assert_eq!(sync2_json["snapshot_updated"], true);

    let traffic2_resp = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/subscriptions/{subscription_id}/traffic"))
                .body(Body::empty())
                .expect("request"),
            &cookie,
        ))
        .await
        .expect("traffic2");
    let traffic2_json = body_to_json(traffic2_resp).await;
    let download2 = traffic2_json["download"].as_u64().expect("download2");
    assert_eq!(
        download2,
        total_download + 3_000_000_000,
        "second sync delta should add exactly 3GB download"
    );
}

// ---------------------------------------------------------------------------
// PROBE-003: Komari traffic sync
// ---------------------------------------------------------------------------

/// Mock Komari panel: returns JSON for `GET /api/nodes` and
/// `GET /api/records/load?uuid=...&load_type=network`.
struct MockKomari {
    net_up: Arc<AtomicU64>,
    net_down: Arc<AtomicU64>,
}

impl MockKomari {
    fn new() -> Self {
        Self {
            net_up: Arc::new(AtomicU64::new(10_000)),
            net_down: Arc::new(AtomicU64::new(20_000)),
        }
    }

    async fn start(&self) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let bind = format!("http://{addr}");

        let net_up = self.net_up.clone();
        let net_down = self.net_down.clone();

        tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                let net_up = net_up.clone();
                let net_down = net_down.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = tokio::io::AsyncReadExt::read(&mut sock, &mut buf)
                        .await
                        .unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]);

                    let body = if request.contains("/api/nodes") {
                        r#"{"status":"success","data":[{"uuid":"uuid-1","name":"Server 1"},{"uuid":"uuid-2","name":"Server 2"}]}"#
                            .to_owned()
                    } else if request.contains("/api/records/load") {
                        let uuid = request
                            .lines()
                            .next()
                            .and_then(|l| l.split("uuid=").nth(1))
                            .and_then(|s| s.split('&').next())
                            .unwrap_or("");
                        let (up, down) = if uuid == "uuid-1" {
                            (
                                net_up.load(Ordering::Relaxed),
                                net_down.load(Ordering::Relaxed),
                            )
                        } else {
                            (5000u64, 6000u64)
                        };
                        format!(
                            r#"{{"status":"success","data":{{"records":[{{"client":"{uuid}","net_total_up":{up},"net_total_down":{down}}}],"count":1,"load_type":"network"}}}}"#
                        )
                    } else {
                        r#"{"status":"error","message":"not found"}"#.to_owned()
                    };

                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    use tokio::io::AsyncWriteExt;
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        bind
    }

    fn bump(&self) {
        self.net_up.fetch_add(3000, Ordering::Relaxed);
        self.net_down.fetch_add(5000, Ordering::Relaxed);
    }
}

/// PROBE-003: Komari traffic sync writes TrafficRecord rows and persists
/// the encrypted counter snapshot. A second sync computes the delta.
#[tokio::test]
async fn probe003_komari_sync_writes_traffic_and_snapshot() {
    let mock = MockKomari::new();
    let endpoint = mock.start().await;

    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;
    let subscription_id = create_subscription(&router, &cookie, &template_id).await;

    let create_body = serde_json::json!({
        "kind": "komari",
        "name": "my-komari",
        "endpoint_url": endpoint,
        "auth_config": "",
        "subscription_id": subscription_id,
    })
    .to_string();
    let resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-sources", &create_body),
            &cookie,
        ))
        .await
        .expect("create source");
    assert_eq!(resp.status(), StatusCode::CREATED);
    let source_json = body_to_json(resp).await;
    let source_id = source_json["source"]["id"]
        .as_str()
        .expect("source id")
        .to_owned();

    let sync_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json(&format!("/api/v1/probe-sources/{source_id}/sync"), ""),
            &cookie,
        ))
        .await
        .expect("sync");
    assert_eq!(sync_resp.status(), StatusCode::OK);
    let sync_json = body_to_json(sync_resp).await;
    assert_eq!(sync_json["samples_written"], 2);
    assert_eq!(sync_json["snapshot_updated"], true);

    let traffic_resp = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/subscriptions/{subscription_id}/traffic"))
                .body(Body::empty())
                .expect("request"),
            &cookie,
        ))
        .await
        .expect("traffic");
    assert_eq!(traffic_resp.status(), StatusCode::OK);
    let traffic_json = body_to_json(traffic_resp).await;
    let total_upload = traffic_json["upload"].as_u64().expect("upload");
    let total_download = traffic_json["download"].as_u64().expect("download");
    assert!(
        total_upload >= 15_000,
        "first sync upload should include both servers: {total_upload}"
    );
    assert!(
        total_download >= 26_000,
        "first sync download should include both servers: {total_download}"
    );

    mock.bump();

    let sync2_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json(&format!("/api/v1/probe-sources/{source_id}/sync"), ""),
            &cookie,
        ))
        .await
        .expect("sync2");
    assert_eq!(sync2_resp.status(), StatusCode::OK);
    let sync2_json = body_to_json(sync2_resp).await;
    assert_eq!(sync2_json["samples_written"], 1);
    assert_eq!(sync2_json["snapshot_updated"], true);

    let traffic2_resp = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/subscriptions/{subscription_id}/traffic"))
                .body(Body::empty())
                .expect("request"),
            &cookie,
        ))
        .await
        .expect("traffic2");
    let traffic2_json = body_to_json(traffic2_resp).await;
    let upload2 = traffic2_json["upload"].as_u64().expect("upload2");
    let download2 = traffic2_json["download"].as_u64().expect("download2");
    assert_eq!(
        upload2,
        total_upload + 3000,
        "second sync delta should add exactly 3000 upload bytes"
    );
    assert_eq!(
        download2,
        total_download + 5000,
        "second sync delta should add exactly 5000 download bytes"
    );
}
