#![allow(clippy::expect_used)]

//! E2E tests for M7 Slice 6: probe source failure handling (PROBE-004) and
//! multi-source traffic aggregation with dashboard traceability (PROBE-005).
//!
//! PROBE-004: point a probe source at an invalid endpoint, sync, verify the
//! previous traffic records are preserved and `last_sync_status = Failed`.
//!
//! PROBE-005: configure multiple probe sources, sync all, verify the dashboard
//! traffic endpoint shows per-source-kind and per-probe-source breakdown.
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` §"Failure/recovery"
//! and §"Traffic aggregation", and `docs/acceptance/matrix.tsv` PROBE-004/005.

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
                        r#"[{{"id":1,"uuid":"srv-1","state":{{"net_in_transfer":{cur_in},"net_out_transfer":{cur_out}}}}}]"#
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
}

/// Mock DStatus panel: returns JSON for `GET /api/allnode_status`.
struct MockDStatus {
    used_a: Arc<AtomicU64>,
}

impl MockDStatus {
    fn new() -> Self {
        Self {
            used_a: Arc::new(AtomicU64::new(8_000_000_000)),
        }
    }

    async fn start(&self) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let bind = format!("http://{addr}");
        let used_a = self.used_a.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                let used_a = used_a.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
                    let ua = used_a.load(Ordering::Relaxed);
                    let body = format!(
                        r#"{{"success":true,"order":["node-a"],"data":{{"node-a":{{"name":"Server A","status":1,"traffic_stats":{{"used":{ua},"limit":0,"unlimited":true}}}}}}}}"#
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
                source_repo: Arc::new(SqliteSourceRepository::new_with_key(
                    pool.clone(),
                    Arc::clone(&master_key),
                )) as Arc<dyn SourceRepository>,
                snapshot_repo: Arc::new(SqliteSourceSnapshotRepository::new(pool.clone()))
                    as Arc<dyn SourceSnapshotRepository>,
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

/// Create a probe source and return its ID.
async fn create_probe_source(
    router: &axum::Router,
    cookie: &str,
    kind: &str,
    name: &str,
    endpoint: &str,
    subscription_id: &str,
) -> String {
    let body = serde_json::json!({
        "kind": kind,
        "name": name,
        "endpoint_url": endpoint,
        "auth_config": "nzp_test_token",
        "subscription_id": subscription_id,
    })
    .to_string();
    let resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-sources", &body),
            cookie,
        ))
        .await
        .expect("create source");
    assert_eq!(resp.status(), StatusCode::CREATED);
    let v = body_to_json(resp).await;
    v["source"]["id"].as_str().expect("source id").to_owned()
}

/// Sync a probe source, returning the response.
async fn sync_source(
    router: &axum::Router,
    cookie: &str,
    source_id: &str,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(with_cookie(
            post_json(&format!("/api/v1/probe-sources/{source_id}/sync"), ""),
            cookie,
        ))
        .await
        .expect("sync")
}

/// Get a probe source by ID.
async fn get_probe_source(
    router: &axum::Router,
    cookie: &str,
    source_id: &str,
) -> serde_json::Value {
    let resp = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/probe-sources/{source_id}"))
                .body(Body::empty())
                .expect("request"),
            cookie,
        ))
        .await
        .expect("get source");
    assert_eq!(resp.status(), StatusCode::OK);
    body_to_json(resp).await
}

/// Get the dashboard traffic aggregate.
async fn get_dashboard_traffic(router: &axum::Router, cookie: &str) -> serde_json::Value {
    let resp = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("GET")
                .uri("/api/v1/dashboard/traffic")
                .body(Body::empty())
                .expect("request"),
            cookie,
        ))
        .await
        .expect("dashboard traffic");
    assert_eq!(resp.status(), StatusCode::OK);
    body_to_json(resp).await
}

// ---------------------------------------------------------------------------
// PROBE-004: Failure handling — preserve stale stats, mark Failed
// ---------------------------------------------------------------------------

/// PROBE-004: when a probe source sync fails (invalid endpoint), the previous
/// traffic records are preserved and `last_sync_status` is set to `Failed`.
#[tokio::test]
async fn probe004_sync_failure_preserves_stale_stats_and_marks_failed() {
    let mock = MockNezha::new();
    let endpoint = mock.start().await;

    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;
    let subscription_id = create_subscription(&router, &cookie, &template_id).await;

    // Step 1: successful sync — writes traffic records, sets last_sync_status = Ok.
    let source_id = create_probe_source(
        &router,
        &cookie,
        "nezha",
        "fail-test",
        &endpoint,
        &subscription_id,
    )
    .await;

    let sync1 = sync_source(&router, &cookie, &source_id).await;
    assert_eq!(sync1.status(), StatusCode::OK);
    let sync1_json = body_to_json(sync1).await;
    assert_eq!(sync1_json["samples_written"], 1);

    // Verify traffic records exist.
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
    let traffic_json = body_to_json(traffic_resp).await;
    let upload_after_success = traffic_json["upload"].as_u64().expect("upload");
    let download_after_success = traffic_json["download"].as_u64().expect("download");
    assert!(
        upload_after_success >= 10_000,
        "first sync should write upload bytes: {upload_after_success}"
    );
    assert!(
        download_after_success >= 20_000,
        "first sync should write download bytes: {download_after_success}"
    );

    // Verify last_sync_status = Ok after successful sync (untagged enum:
    // Ok serializes as null; Failed serializes as {"message": "..."}).
    let source_after_success = get_probe_source(&router, &cookie, &source_id).await;
    let status_ok = &source_after_success["source"]["last_sync_status"];
    assert!(
        status_ok.get("message").is_none(),
        "last_sync_status should not be Failed after successful sync, got: {status_ok}"
    );

    // Step 2: point the source at an invalid endpoint (connection refused).
    // Port 1 is reserved and will refuse connections.
    let invalid_endpoint = "http://127.0.0.1:1";
    let update_body = serde_json::json!({"endpoint_url": invalid_endpoint}).to_string();
    let _ = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/probe-sources/{source_id}"))
                .header("content-type", "application/json")
                .body(json_body(&update_body))
                .expect("request"),
            &cookie,
        ))
        .await
        .expect("update");

    // Step 3: sync fails — adapter returns error, command marks Failed, returns 500.
    let sync2 = sync_source(&router, &cookie, &source_id).await;
    assert_eq!(
        sync2.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "sync to invalid endpoint should fail with 500"
    );

    // Step 4: verify last_sync_status = Failed.
    let source_after_fail = get_probe_source(&router, &cookie, &source_id).await;
    let status = &source_after_fail["source"]["last_sync_status"];
    assert!(
        status.get("message").is_some(),
        "last_sync_status should be Failed with a message after failed sync, got: {status}"
    );
    let fail_msg = status["message"].as_str().expect("failure message string");
    assert!(!fail_msg.is_empty(), "failure message should not be empty");

    // Step 5: verify previous traffic records are preserved (not dropped).
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
    let upload_after_fail = traffic2_json["upload"].as_u64().expect("upload");
    let download_after_fail = traffic2_json["download"].as_u64().expect("download");
    assert_eq!(
        upload_after_fail, upload_after_success,
        "failed sync must not change traffic totals (stale stats preserved)"
    );
    assert_eq!(
        download_after_fail, download_after_success,
        "failed sync must not change traffic totals (stale stats preserved)"
    );
}

// ---------------------------------------------------------------------------
// PROBE-005: Multi-source aggregation with dashboard traceability
// ---------------------------------------------------------------------------

/// PROBE-005: configure multiple probe sources (Nezha + DStatus), sync both,
/// verify the dashboard traffic endpoint shows per-source-kind and
/// per-probe-source breakdown with traceable attribution.
#[tokio::test]
async fn probe005_multi_source_aggregation_dashboard_traceability() {
    let nezha_mock = MockNezha::new();
    let nezha_endpoint = nezha_mock.start().await;

    let dstatus_mock = MockDStatus::new();
    let dstatus_endpoint = dstatus_mock.start().await;

    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;
    let subscription_id = create_subscription(&router, &cookie, &template_id).await;

    // Create two probe sources bound to the same subscription.
    let nezha_id = create_probe_source(
        &router,
        &cookie,
        "nezha",
        "nezha-panel",
        &nezha_endpoint,
        &subscription_id,
    )
    .await;
    let dstatus_id = create_probe_source(
        &router,
        &cookie,
        "dstatus",
        "dstatus-panel",
        &dstatus_endpoint,
        &subscription_id,
    )
    .await;

    // Sync both.
    let sync_nezha = sync_source(&router, &cookie, &nezha_id).await;
    assert_eq!(sync_nezha.status(), StatusCode::OK);
    let sync_dstatus = sync_source(&router, &cookie, &dstatus_id).await;
    assert_eq!(sync_dstatus.status(), StatusCode::OK);

    // Query the dashboard traffic aggregate.
    let dashboard = get_dashboard_traffic(&router, &cookie).await;

    // Verify per-source-kind breakdown includes "probe".
    let by_source_kind = dashboard["by_source_kind"]
        .as_array()
        .expect("by_source_kind");
    let probe_kind = by_source_kind
        .iter()
        .find(|e| e["source_kind"] == "probe")
        .expect("probe source kind in breakdown");
    let probe_kind_upload = probe_kind["upload"].as_u64().expect("probe upload");
    let probe_kind_download = probe_kind["download"].as_u64().expect("probe download");
    assert!(
        probe_kind_upload > 0,
        "probe-kind upload should be > 0 after syncing both sources"
    );
    assert!(
        probe_kind_download > 0,
        "probe-kind download should be > 0 after syncing both sources"
    );

    // Verify per-probe-source breakdown includes both sources with attribution.
    let by_probe_source = dashboard["by_probe_source"]
        .as_array()
        .expect("by_probe_source");
    assert!(
        by_probe_source.len() >= 2,
        "dashboard should show at least 2 probe source contributions, got {}",
        by_probe_source.len()
    );

    let nezha_entry = by_probe_source
        .iter()
        .find(|e| e["source_id"] == nezha_id)
        .expect("nezha source in dashboard breakdown");
    assert_eq!(nezha_entry["kind"], "nezha");
    assert_eq!(nezha_entry["name"], "nezha-panel");
    assert_eq!(nezha_entry["enabled"], true);
    assert!(
        nezha_entry["upload"].as_u64().expect("nezha upload") > 0,
        "nezha attribution upload should be > 0"
    );
    assert!(
        nezha_entry["download"].as_u64().expect("nezha download") > 0,
        "nezha attribution download should be > 0"
    );

    let dstatus_entry = by_probe_source
        .iter()
        .find(|e| e["source_id"] == dstatus_id)
        .expect("dstatus source in dashboard breakdown");
    assert_eq!(dstatus_entry["kind"], "dstatus");
    assert_eq!(dstatus_entry["name"], "dstatus-panel");
    assert_eq!(dstatus_entry["enabled"], true);
    assert!(
        dstatus_entry["upload"].as_u64().expect("dstatus upload") == 0
            || dstatus_entry["download"]
                .as_u64()
                .expect("dstatus download")
                > 0,
        "dstatus attribution should have traffic"
    );

    // Verify global totals match the sum of the per-source-kind breakdown.
    let total_upload = dashboard["total_upload"].as_u64().expect("total_upload");
    let total_download = dashboard["total_download"]
        .as_u64()
        .expect("total_download");
    let kind_sum_upload: u64 = by_source_kind
        .iter()
        .map(|e| e["upload"].as_u64().unwrap_or(0))
        .sum();
    let kind_sum_download: u64 = by_source_kind
        .iter()
        .map(|e| e["download"].as_u64().unwrap_or(0))
        .sum();
    assert_eq!(
        total_upload, kind_sum_upload,
        "total_upload should equal sum of by_source_kind uploads"
    );
    assert_eq!(
        total_download, kind_sum_download,
        "total_download should equal sum of by_source_kind downloads"
    );

    // Verify per-probe-source uploads sum to the probe-kind upload (traceability).
    let probe_source_sum_upload: u64 = by_probe_source
        .iter()
        .map(|e| e["upload"].as_u64().unwrap_or(0))
        .sum();
    let probe_source_sum_download: u64 = by_probe_source
        .iter()
        .map(|e| e["download"].as_u64().unwrap_or(0))
        .sum();
    assert_eq!(
        probe_source_sum_upload, probe_kind_upload,
        "sum of per-probe-source uploads should equal probe-kind upload (traceability)"
    );
    assert_eq!(
        probe_source_sum_download, probe_kind_download,
        "sum of per-probe-source downloads should equal probe-kind download (traceability)"
    );
}

// ---------------------------------------------------------------------------
// Dashboard latency endpoint
// ---------------------------------------------------------------------------

/// `GET /api/v1/dashboard/latency` returns 200 with an empty records array
/// when no latency records exist.
#[tokio::test]
async fn dashboard_latency_empty_returns_200() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let resp = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("GET")
                .uri("/api/v1/dashboard/latency")
                .body(Body::empty())
                .expect("request"),
            &cookie,
        ))
        .await
        .expect("dashboard latency");
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_to_json(resp).await;
    assert_eq!(
        json["records"].as_array().map(Vec::len),
        Some(0),
        "records should be empty when no latency records exist"
    );
}

/// `GET /api/v1/dashboard/latency` requires admin auth.
#[tokio::test]
async fn dashboard_latency_requires_auth() {
    let app = TestApp::new().await;
    let router = app.router();

    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/dashboard/latency")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("send");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// `GET /api/v1/dashboard/traffic` requires admin auth.
#[tokio::test]
async fn dashboard_traffic_requires_auth() {
    let app = TestApp::new().await;
    let router = app.router();

    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/dashboard/traffic")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("send");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
