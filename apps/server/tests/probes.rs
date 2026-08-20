#![allow(clippy::expect_used)]

//! Integration tests for M7 Slice 1: probe source CRUD, TCP latency probe
//! (NODE-012), QUIC handshake probe (NODE-013), UDP no-response handling
//! (NODE-014), and latency query.
//!
//! Covers:
//! - `POST/GET/PUT/DELETE /api/v1/probe-sources/*` (probe source CRUD)
//! - `POST /api/v1/probe-runs` + `GET /api/v1/probe-runs/{id}` (NODE-012 TCP RTT,
//!   NODE-013 QUIC handshake RTT, NODE-014 UDP no-response)
//! - `GET /api/v1/nodes/{id}/latency` (latency record query)
//! - Error classification on a closed port (refused) and silent UDP (timeout)
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` §"Latency probe
//! model".

use std::str::FromStr;
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
                job_supervisor: Arc::new(deve_sub_application::JobSupervisor::new()),
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

fn put_json(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("PUT")
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
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .expect("body");
    serde_json::from_slice(&body).expect("json")
}

/// Import a single trojan node and return its ULID.
async fn import_node(router: &axum::Router, cookie: &str, uri: &str) -> String {
    let body = format!(r#"{{"content":"{uri}","source_type":"uri_list"}}"#);
    let resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/nodes/import", &body),
            cookie,
        ))
        .await
        .expect("import");
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_to_json(resp).await;
    // ImportOutcomeDto uses #[serde(tag = "status", content = "data")],
    // so Inserted(ULID) serializes as {"status":"inserted","data":"ULID"}.
    assert_eq!(json["outcomes"][0]["status"], "inserted");
    json["outcomes"][0]["data"]
        .as_str()
        .expect("data ULID")
        .to_owned()
}

/// Create a probe source and return its ULID.
#[tokio::test]
async fn create_probe_source() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/probe-sources",
                r#"{"kind":"nezha","name":"my-nezha","endpoint_url":"https://nezha.example.com","auth_config":"token-abc"}"#,
            ),
            &cookie,
        ))
        .await
        .expect("create");

    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    assert_eq!(json["source"]["kind"], "nezha");
    assert_eq!(json["source"]["name"], "my-nezha");
    assert_eq!(json["source"]["endpoint_url"], "https://nezha.example.com");
    assert_eq!(json["source"]["has_auth"], true);
    assert_eq!(json["source"]["enabled"], true);
    assert!(json["source"]["id"].as_str().is_some());
}

/// Duplicate probe source name is rejected with 409.
#[tokio::test]
async fn create_probe_source_duplicate_name() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let body = r#"{"kind":"nezha","name":"dup","endpoint_url":"https://a.example.com"}"#;
    let r1 = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-sources", body),
            &cookie,
        ))
        .await
        .expect("first");
    assert_eq!(r1.status(), StatusCode::CREATED);

    let r2 = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-sources", body),
            &cookie,
        ))
        .await
        .expect("second");
    assert_eq!(r2.status(), StatusCode::CONFLICT);
}

/// List probe sources with pagination.
#[tokio::test]
async fn list_probe_sources() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    for i in 0..3 {
        let body = format!(
            r#"{{"kind":"komari","name":"k{i}","endpoint_url":"https://k{i}.example.com"}}"#
        );
        let _ = router
            .clone()
            .oneshot(with_cookie(
                post_json("/api/v1/probe-sources", &body),
                &cookie,
            ))
            .await
            .expect("create");
    }

    let response = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/probe-sources?limit=2"), &cookie))
        .await
        .expect("list");

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert_eq!(json["sources"].as_array().expect("sources array").len(), 2);
    assert!(json["next_cursor"].as_str().is_some());
}

/// Update a probe source.
#[tokio::test]
async fn update_probe_source() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let create_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/probe-sources",
                r#"{"kind":"dstatus","name":"ds","endpoint_url":"https://ds.example.com"}"#,
            ),
            &cookie,
        ))
        .await
        .expect("create");
    let create_json = body_to_json(create_resp).await;
    let id = create_json["source"]["id"]
        .as_str()
        .expect("source id")
        .to_owned();

    let response = router
        .clone()
        .oneshot(with_cookie(
            put_json(
                &format!("/api/v1/probe-sources/{id}"),
                r#"{"name":"ds-renamed","enabled":false}"#,
            ),
            &cookie,
        ))
        .await
        .expect("update");

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert_eq!(json["source"]["name"], "ds-renamed");
    assert_eq!(json["source"]["enabled"], false);
}

/// Delete a probe source.
#[tokio::test]
async fn delete_probe_source() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let create_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/probe-sources",
                r#"{"kind":"nezha","name":"to-delete","endpoint_url":"https://n.example.com"}"#,
            ),
            &cookie,
        ))
        .await
        .expect("create");
    let id = body_to_json(create_resp).await["source"]["id"]
        .as_str()
        .expect("source id")
        .to_owned();

    let del_resp = router
        .clone()
        .oneshot(with_cookie(
            delete(&format!("/api/v1/probe-sources/{id}")),
            &cookie,
        ))
        .await
        .expect("delete");
    assert_eq!(del_resp.status(), StatusCode::OK);

    let get_resp = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/probe-sources/{id}")),
            &cookie,
        ))
        .await
        .expect("get after delete");
    assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
}

/// NODE-012: TCP connect probe against a live endpoint records RTT and Ok.
#[tokio::test]
async fn tcp_probe_records_rtt_for_live_endpoint() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    // Bind a real TCP listener to get a guaranteed-open port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    // Keep the listener alive in the background.
    let listener_handle = tokio::spawn(async move {
        loop {
            if listener.accept().await.is_err() {
                break;
            }
        }
    });

    let node_uri =
        format!("trojan://TEST_PASSWORD@127.0.0.1:{port}?sni=example.com&type=tcp#live-node");
    let node_id = import_node(&router, &cookie, &node_uri).await;

    // Start a TCP probe run targeting the imported node.
    let run_body = format!(r#"{{"probe_type":"tcp_connect","node_ids":["{node_id}"]}}"#);
    let run_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-runs", &run_body),
            &cookie,
        ))
        .await
        .expect("run");
    assert_eq!(run_resp.status(), StatusCode::CREATED);
    let run_json = body_to_json(run_resp).await;
    let run_id = run_json["run"]["id"].as_str().expect("run id").to_owned();

    // Poll until the run reaches a terminal status (max ~10s).
    let mut final_json = serde_json::Value::Null;
    for _ in 0..100 {
        let resp = router
            .clone()
            .oneshot(with_cookie(
                get(&format!("/api/v1/probe-runs/{run_id}")),
                &cookie,
            ))
            .await
            .expect("poll");
        let json = body_to_json(resp).await;
        let status = json["run"]["status"].as_str().unwrap_or("");
        if status == "completed" || status == "cancelled" || status == "failed" {
            final_json = json;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    assert_eq!(final_json["run"]["status"], "completed");
    let results = final_json["run"]["results"]
        .as_array()
        .expect("results array");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["node_id"], node_id);
    assert!(
        results[0]["rtt_ms"].as_u64().is_some(),
        "rtt_ms should be Some for a live endpoint"
    );
    assert_eq!(results[0]["error_class"], "ok");
    assert_eq!(results[0]["skipped"], false);

    // Query latency records for the node.
    let lat_resp = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/nodes/{node_id}/latency")),
            &cookie,
        ))
        .await
        .expect("latency");
    assert_eq!(lat_resp.status(), StatusCode::OK);
    let lat_json = body_to_json(lat_resp).await;
    let records = lat_json["records"].as_array().expect("records array");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["node_id"], node_id);
    assert_eq!(records[0]["probe_type"], "tcp_connect");
    assert!(records[0]["rtt_ms"].as_u64().is_some());

    listener_handle.abort();
}

/// TCP probe against a closed port classifies the error as refused.
#[tokio::test]
async fn tcp_probe_closed_port_classified_as_refused() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    // Bind and immediately drop to get a guaranteed-closed port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    let node_uri =
        format!("trojan://TEST_PASSWORD@127.0.0.1:{port}?sni=example.com&type=tcp#dead-node");
    let node_id = import_node(&router, &cookie, &node_uri).await;

    let run_body = format!(r#"{{"probe_type":"tcp_connect","node_ids":["{node_id}"]}}"#);
    let run_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-runs", &run_body),
            &cookie,
        ))
        .await
        .expect("run");
    assert_eq!(run_resp.status(), StatusCode::CREATED);
    let run_id = body_to_json(run_resp).await["run"]["id"]
        .as_str()
        .expect("run id")
        .to_owned();

    // Poll for completion.
    let mut final_status = String::new();
    let mut final_json = serde_json::Value::Null;
    for _ in 0..100 {
        let resp = router
            .clone()
            .oneshot(with_cookie(
                get(&format!("/api/v1/probe-runs/{run_id}")),
                &cookie,
            ))
            .await
            .expect("poll");
        let json = body_to_json(resp).await;
        let status = json["run"]["status"].as_str().unwrap_or("");
        if status == "completed" || status == "cancelled" || status == "failed" {
            final_status = status.to_owned();
            final_json = json;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    assert_eq!(final_status, "completed");
    let results = final_json["run"]["results"]
        .as_array()
        .expect("results array");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["node_id"], node_id);
    assert!(
        results[0]["rtt_ms"].is_null(),
        "rtt_ms should be null for a closed port"
    );
    assert_eq!(results[0]["error_class"], "refused");
}

/// Creating a probe run with an empty node list is rejected.
#[tokio::test]
async fn probe_run_empty_node_list_rejected() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/probe-runs",
                r#"{"probe_type":"tcp_connect","node_ids":[]}"#,
            ),
            &cookie,
        ))
        .await
        .expect("run");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// A self-signed ECDSA P-256 certificate for `127.0.0.1`, DER-encoded.
/// Generated once and embedded so tests need no `rcgen` dependency or
/// runtime cert generation. Safe to commit: it is a test-only cert with no
/// private key embedded alongside the secret in production code paths.
const QUIC_TEST_CERT_DER: &[u8] = include_bytes!("fixtures/cert.der");
const QUIC_TEST_KEY_DER: &[u8] = include_bytes!("fixtures/key.der");

/// Spawn a real QUIC server on `127.0.0.1:0` that completes handshakes.
/// Returns the bound port. The server runs until the returned handle is
/// aborted.
fn spawn_quic_server() -> (u16, tokio::task::JoinHandle<()>) {
    use std::net::SocketAddr;

    let cert = rustls::pki_types::CertificateDer::from(QUIC_TEST_CERT_DER.to_vec());
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(QUIC_TEST_KEY_DER.to_vec().into());

    let server_config =
        quinn::ServerConfig::with_single_cert(vec![cert], key).expect("server config");
    let endpoint = quinn::Endpoint::server(server_config, SocketAddr::from(([127, 0, 0, 1], 0)))
        .expect("bind");

    let port = endpoint.local_addr().expect("addr").port();
    let handle = tokio::spawn(async move {
        // Await the Connecting future to drive the handshake to completion.
        // We do not exchange application data — the probe only measures
        // handshake RTT, so the connection is dropped immediately after.
        while let Some(incoming) = endpoint.accept().await {
            if let Ok(connecting) = incoming.accept() {
                let _ = connecting.await;
            }
        }
    });
    (port, handle)
}

/// NODE-013: QUIC handshake probe against a live QUIC endpoint records RTT
/// and `error_class = "ok"`.
#[tokio::test]
async fn quic_probe_records_rtt_for_live_endpoint() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let (port, _server_handle) = spawn_quic_server();

    let node_uri =
        format!("hysteria2://TEST_PASSWORD@127.0.0.1:{port}?sni=127.0.0.1#quic-live-node");
    let node_id = import_node(&router, &cookie, &node_uri).await;

    let run_body = format!(r#"{{"probe_type":"quic_handshake","node_ids":["{node_id}"]}}"#);
    let run_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-runs", &run_body),
            &cookie,
        ))
        .await
        .expect("run");
    assert_eq!(run_resp.status(), StatusCode::CREATED);
    let run_id = body_to_json(run_resp).await["run"]["id"]
        .as_str()
        .expect("run id")
        .to_owned();

    // Poll until terminal (max ~10s; handshake should complete in <1s).
    let mut final_json = serde_json::Value::Null;
    for _ in 0..100 {
        let resp = router
            .clone()
            .oneshot(with_cookie(
                get(&format!("/api/v1/probe-runs/{run_id}")),
                &cookie,
            ))
            .await
            .expect("poll");
        let json = body_to_json(resp).await;
        let status = json["run"]["status"].as_str().unwrap_or("");
        if status == "completed" || status == "cancelled" || status == "failed" {
            final_json = json;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    assert_eq!(final_json["run"]["status"], "completed");
    let results = final_json["run"]["results"]
        .as_array()
        .expect("results array");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["node_id"], node_id);
    assert!(
        results[0]["rtt_ms"].as_u64().is_some(),
        "rtt_ms should be Some for a live QUIC endpoint"
    );
    assert_eq!(results[0]["error_class"], "ok");
    assert_eq!(results[0]["skipped"], false);

    let lat_resp = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/nodes/{node_id}/latency")),
            &cookie,
        ))
        .await
        .expect("latency");
    assert_eq!(lat_resp.status(), StatusCode::OK);
    let lat_json = body_to_json(lat_resp).await;
    let records = lat_json["records"].as_array().expect("records array");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["probe_type"], "quic_handshake");
    assert!(records[0]["rtt_ms"].as_u64().is_some());
}

/// NODE-014 (regression): a non-responsive UDP endpoint produces
/// `rtt_ms = None` + `error_class = "timeout"` — no fake latency, no
/// auto-kill. We bind a UDP socket that never responds to QUIC packets,
/// so the handshake times out within the probe deadline.
#[tokio::test]
async fn quic_probe_silent_udp_endpoint_times_out() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    // Bind a UDP socket that silently absorbs packets (never responds).
    let silent_socket = tokio::net::UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind udp");
    let port = silent_socket.local_addr().expect("addr").port();
    // Keep the socket alive in the background; drain incoming packets but
    // never reply.
    let socket_handle = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        loop {
            if silent_socket.recv_from(&mut buf).await.is_err() {
                break;
            }
        }
    });

    let node_uri =
        format!("hysteria2://TEST_PASSWORD@127.0.0.1:{port}?sni=127.0.0.1#silent-udp-node");
    let node_id = import_node(&router, &cookie, &node_uri).await;

    let run_body = format!(r#"{{"probe_type":"quic_handshake","node_ids":["{node_id}"]}}"#);
    let run_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-runs", &run_body),
            &cookie,
        ))
        .await
        .expect("run");
    assert_eq!(run_resp.status(), StatusCode::CREATED);
    let run_id = body_to_json(run_resp).await["run"]["id"]
        .as_str()
        .expect("run id")
        .to_owned();

    // Poll until terminal. The probe timeout is 5s; allow up to ~15s for
    // scheduling + DB writes.
    let mut final_json = serde_json::Value::Null;
    for _ in 0..150 {
        let resp = router
            .clone()
            .oneshot(with_cookie(
                get(&format!("/api/v1/probe-runs/{run_id}")),
                &cookie,
            ))
            .await
            .expect("poll");
        let json = body_to_json(resp).await;
        let status = json["run"]["status"].as_str().unwrap_or("");
        if status == "completed" || status == "cancelled" || status == "failed" {
            final_json = json;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    assert_eq!(final_json["run"]["status"], "completed");
    let results = final_json["run"]["results"]
        .as_array()
        .expect("results array");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["node_id"], node_id);
    assert!(
        results[0]["rtt_ms"].is_null(),
        "rtt_ms must be None for a silent UDP endpoint (no fake latency)"
    );
    assert_eq!(
        results[0]["error_class"], "timeout",
        "silent UDP must classify as timeout, not a fake RTT"
    );
    assert_eq!(results[0]["skipped"], false);

    let lat_resp = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/nodes/{node_id}/latency")),
            &cookie,
        ))
        .await
        .expect("latency");
    assert_eq!(lat_resp.status(), StatusCode::OK);
    let lat_json = body_to_json(lat_resp).await;
    let records = lat_json["records"].as_array().expect("records array");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["probe_type"], "quic_handshake");
    assert!(
        records[0]["rtt_ms"].is_null(),
        "persisted rtt_ms must be null"
    );
    assert_eq!(records[0]["error_class"], "timeout");

    socket_handle.abort();
}

/// NODE-015: real-proxy probe run via the API produces correctly-structured
/// results. The adapter's 14 unit tests verify protocol-level round-trips for
/// all 7 P0 protocols; this E2E test verifies the API → runner → adapter
/// dispatch → result path. A node with an unreachable endpoint produces
/// `error_class = "refused"` and `rtt_ms = null`.
#[tokio::test]
async fn real_proxy_probe_run_via_api_produces_results() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    let node_uri =
        format!("trojan://TEST_PASSWORD@127.0.0.1:{port}?sni=example.com&type=tcp#real-proxy-node");
    let node_id = import_node(&router, &cookie, &node_uri).await;

    let run_body = format!(r#"{{"probe_type":"real_proxy","node_ids":["{node_id}"]}}"#);
    let run_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-runs", &run_body),
            &cookie,
        ))
        .await
        .expect("run");
    assert_eq!(run_resp.status(), StatusCode::CREATED);
    let run_id = body_to_json(run_resp).await["run"]["id"]
        .as_str()
        .expect("run id")
        .to_owned();

    let mut final_json = serde_json::Value::Null;
    for _ in 0..100 {
        let resp = router
            .clone()
            .oneshot(with_cookie(
                get(&format!("/api/v1/probe-runs/{run_id}")),
                &cookie,
            ))
            .await
            .expect("poll");
        let json = body_to_json(resp).await;
        let status = json["run"]["status"].as_str().unwrap_or("");
        if status == "completed" || status == "cancelled" || status == "failed" {
            final_json = json;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    assert_eq!(final_json["run"]["status"], "completed");
    let results = final_json["run"]["results"]
        .as_array()
        .expect("results array");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["node_id"], node_id);
    assert!(
        results[0]["rtt_ms"].is_null(),
        "rtt_ms should be null for a refused connection"
    );
    assert_eq!(results[0]["error_class"], "refused");
    assert_eq!(results[0]["skipped"], false);
}

/// NODE-016: batch cancel of an in-flight probe run. In-flight probes are
/// aborted (timeout) and pending probes are skipped. The run reaches
/// `Cancelled` status with partial results.
#[tokio::test]
async fn batch_cancel_probe_run_aborts_inflight_and_skips_pending() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    // 192.0.2.0/24 is TEST-NET-1 (RFC 5737) — documentation-only, unroutable.
    // TCP connect hangs until the probe timeout fires, giving us a window to
    // cancel while probes are in-flight. Each node uses a distinct IP so the
    // import dedup (which keys on endpoint) treats them as unique.
    let mut node_ids = Vec::new();
    for i in 1..=35 {
        let uri = format!("trojan://pw@192.0.2.{i}:80?sni=example.com#cancel-node-{i}");
        let id = import_node(&router, &cookie, &uri).await;
        node_ids.push(id);
    }

    let ids_json = node_ids
        .iter()
        .map(|id| format!("\"{id}\""))
        .collect::<Vec<_>>()
        .join(",");
    let run_body = format!(r#"{{"probe_type":"tcp_connect","node_ids":[{ids_json}]}}"#);
    let run_resp = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-runs", &run_body),
            &cookie,
        ))
        .await
        .expect("run");
    assert_eq!(run_resp.status(), StatusCode::CREATED);
    let run_id = body_to_json(run_resp).await["run"]["id"]
        .as_str()
        .expect("run id")
        .to_owned();

    let cancel_resp = router
        .clone()
        .oneshot(with_cookie(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/probe-runs/{run_id}/cancel"))
                .body(Body::empty())
                .expect("cancel request"),
            &cookie,
        ))
        .await
        .expect("cancel");
    assert_eq!(cancel_resp.status(), StatusCode::OK);

    let mut final_json = serde_json::Value::Null;
    for _ in 0..150 {
        let resp = router
            .clone()
            .oneshot(with_cookie(
                get(&format!("/api/v1/probe-runs/{run_id}")),
                &cookie,
            ))
            .await
            .expect("poll");
        let json = body_to_json(resp).await;
        let status = json["run"]["status"].as_str().unwrap_or("");
        if status == "completed" || status == "cancelled" || status == "failed" {
            final_json = json;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    assert_eq!(
        final_json["run"]["status"], "cancelled",
        "run must be cancelled, not completed"
    );
    let results = final_json["run"]["results"]
        .as_array()
        .expect("results array");
    assert_eq!(results.len(), 35);

    for r in results {
        assert!(
            r["rtt_ms"].is_null(),
            "rtt_ms must be null for unroutable endpoint"
        );
    }

    let skipped_count = results.iter().filter(|r| r["skipped"] == true).count();
    assert!(
        skipped_count > 0,
        "expected at least 1 skipped probe, got {skipped_count}"
    );
}
