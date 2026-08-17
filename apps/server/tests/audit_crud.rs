#![allow(clippy::expect_used)]

//! Integration tests for CRUD audit wiring (AUDIT-003).
//!
//! Verifies that source, subscription, template, and probe-source CRUD
//! operations produce audit_log entries with correct actor, action, and
//! target.

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

fn put_with_cookie(uri: &str, body: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(uri)
        .header("content-type", "application/json")
        .header("cookie", cookie)
        .body(json_body(body))
        .expect("request")
}

fn delete_with_cookie(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .header("cookie", cookie)
        .body(Body::empty())
        .expect("request")
}

fn extract_cookie(response: &axum::response::Response) -> Option<String> {
    let cookies = response.headers().get("set-cookie")?.to_str().ok()?;
    let part = cookies
        .split(';')
        .find(|s| s.trim().starts_with("deve_sub_session="))?;
    Some(part.trim().to_owned())
}

async fn setup_and_login(app: &TestApp) -> (axum::Router, String) {
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
    (router, cookie)
}

async fn fetch_audit_actions(router: &axum::Router, cookie: &str) -> Vec<String> {
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/audit-logs?limit=200", cookie))
        .await
        .expect("audit query");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    json["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .map(|e| e["action"].as_str().expect("action").to_owned())
        .collect()
}

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

/// AUDIT-003: Source CRUD operations are audited.
#[tokio::test]
async fn source_crud_audited() {
    let app = TestApp::new().await;
    let (router, cookie) = setup_and_login(&app).await;

    // Create.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/sources",
            r#"{"name":"my-sub","source_type":"auto","url":"https://example.com/sub","auto_update":true,"update_interval_secs":1800,"keep_on_fail":true}"#,
            &cookie,
        ))
        .await
        .expect("create source");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let source_id = json["source"]["id"].as_str().expect("source id").to_owned();

    // Update.
    let response = router
        .clone()
        .oneshot(put_with_cookie(
            &format!("/api/v1/sources/{source_id}"),
            r#"{"name":"my-sub-renamed","source_type":"auto","url":"https://example.com/sub","auto_update":true,"update_interval_secs":3600,"enabled":true,"keep_on_fail":true}"#,
            &cookie,
        ))
        .await
        .expect("update source");
    assert_eq!(response.status(), StatusCode::OK);

    // Delete.
    let response = router
        .clone()
        .oneshot(delete_with_cookie(
            &format!("/api/v1/sources/{source_id}"),
            &cookie,
        ))
        .await
        .expect("delete source");
    assert_eq!(response.status(), StatusCode::OK);

    let actions = fetch_audit_actions(&router, &cookie).await;
    assert!(
        actions.contains(&"source.create".to_owned()),
        "source.create should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"source.update".to_owned()),
        "source.update should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"source.delete".to_owned()),
        "source.delete should be audited: {actions:?}"
    );
}

/// AUDIT-003: Template CRUD + rollback operations are audited.
#[tokio::test]
async fn template_crud_audited() {
    let app = TestApp::new().await;
    let (router, cookie) = setup_and_login(&app).await;

    let create_body = serde_json::json!({
        "name": "test-tpl",
        "description": "test",
        "spec_yaml": VALID_SPEC_YAML,
    })
    .to_string();

    // Create.
    let response = router
        .clone()
        .oneshot(post_with_cookie("/api/v1/templates", &create_body, &cookie))
        .await
        .expect("create template");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let template_id = json["template"]["id"]
        .as_str()
        .expect("template id")
        .to_owned();
    let version_id = json["version"]["id"]
        .as_str()
        .expect("version id")
        .to_owned();

    // Update (creates a new version).
    let update_body = serde_json::json!({
        "name": "test-tpl-v2",
        "description": "updated",
        "spec_yaml": VALID_SPEC_YAML,
    })
    .to_string();
    let response = router
        .clone()
        .oneshot(put_with_cookie(
            &format!("/api/v1/templates/{template_id}"),
            &update_body,
            &cookie,
        ))
        .await
        .expect("update template");
    assert_eq!(response.status(), StatusCode::OK);

    // Rollback to v1.
    let rollback_body = format!(r#"{{"version_id":"{version_id}"}}"#);
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            &format!("/api/v1/templates/{template_id}/rollback"),
            &rollback_body,
            &cookie,
        ))
        .await
        .expect("rollback template");
    assert_eq!(response.status(), StatusCode::OK);

    // Delete.
    let response = router
        .clone()
        .oneshot(delete_with_cookie(
            &format!("/api/v1/templates/{template_id}"),
            &cookie,
        ))
        .await
        .expect("delete template");
    assert_eq!(response.status(), StatusCode::OK);

    let actions = fetch_audit_actions(&router, &cookie).await;
    assert!(
        actions.contains(&"template.create".to_owned()),
        "template.create should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"template.update".to_owned()),
        "template.update should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"template.rollback".to_owned()),
        "template.rollback should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"template.delete".to_owned()),
        "template.delete should be audited: {actions:?}"
    );
}

/// AUDIT-003: Subscription CRUD + token rotation are audited.
#[tokio::test]
async fn subscription_crud_audited() {
    let app = TestApp::new().await;
    let (router, cookie) = setup_and_login(&app).await;

    // Need a template first for subscription creation.
    let tpl_body = serde_json::json!({
        "name": "sub-tpl",
        "description": "",
        "spec_yaml": VALID_SPEC_YAML,
    })
    .to_string();
    let response = router
        .clone()
        .oneshot(post_with_cookie("/api/v1/templates", &tpl_body, &cookie))
        .await
        .expect("create template");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let template_id = json["template"]["id"]
        .as_str()
        .expect("template id")
        .to_owned();

    // Create subscription.
    let create_body = serde_json::json!({
        "name": "my-sub",
        "slug": "my-sub",
        "template_id": template_id,
        "profile": "mihomo",
        "node_selection": {"mode": "dynamic"},
    })
    .to_string();
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/subscriptions",
            &create_body,
            &cookie,
        ))
        .await
        .expect("create subscription");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let sub_id = json["subscription"]["id"]
        .as_str()
        .expect("subscription id")
        .to_owned();

    // Update subscription.
    let update_body = serde_json::json!({
        "name": "my-sub-renamed",
        "slug": "my-sub-renamed",
        "profile": "mihomo",
        "node_selection": {"mode": "dynamic"},
        "enabled": true,
    })
    .to_string();
    let response = router
        .clone()
        .oneshot(put_with_cookie(
            &format!("/api/v1/subscriptions/{sub_id}"),
            &update_body,
            &cookie,
        ))
        .await
        .expect("update subscription");
    assert_eq!(response.status(), StatusCode::OK);

    // Rotate token.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            &format!("/api/v1/subscriptions/{sub_id}/rotate-token"),
            r#"{"grace_seconds":0}"#,
            &cookie,
        ))
        .await
        .expect("rotate token");
    assert_eq!(response.status(), StatusCode::OK);

    // Delete subscription.
    let response = router
        .clone()
        .oneshot(delete_with_cookie(
            &format!("/api/v1/subscriptions/{sub_id}"),
            &cookie,
        ))
        .await
        .expect("delete subscription");
    assert_eq!(response.status(), StatusCode::OK);

    let actions = fetch_audit_actions(&router, &cookie).await;
    assert!(
        actions.contains(&"subscription.create".to_owned()),
        "subscription.create should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"subscription.update".to_owned()),
        "subscription.update should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"subscription.token.rotate".to_owned()),
        "subscription.token.rotate should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"subscription.delete".to_owned()),
        "subscription.delete should be audited: {actions:?}"
    );
}

/// AUDIT-003: Probe source CRUD operations are audited.
#[tokio::test]
async fn probe_source_crud_audited() {
    let app = TestApp::new().await;
    let (router, cookie) = setup_and_login(&app).await;

    // Create.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/probe-sources",
            r#"{"kind":"nezha","name":"my-nezha","endpoint_url":"https://nezha.example.com","auth_config":"token-abc"}"#,
            &cookie,
        ))
        .await
        .expect("create probe source");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let probe_id = json["source"]["id"]
        .as_str()
        .expect("probe source id")
        .to_owned();

    // Update.
    let response = router
        .clone()
        .oneshot(put_with_cookie(
            &format!("/api/v1/probe-sources/{probe_id}"),
            r#"{"name":"my-nezha-renamed","enabled":true}"#,
            &cookie,
        ))
        .await
        .expect("update probe source");
    assert_eq!(response.status(), StatusCode::OK);

    // Delete.
    let response = router
        .clone()
        .oneshot(delete_with_cookie(
            &format!("/api/v1/probe-sources/{probe_id}"),
            &cookie,
        ))
        .await
        .expect("delete probe source");
    assert_eq!(response.status(), StatusCode::OK);

    let actions = fetch_audit_actions(&router, &cookie).await;
    assert!(
        actions.contains(&"probe.source.create".to_owned()),
        "probe.source.create should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"probe.source.update".to_owned()),
        "probe.source.update should be audited: {actions:?}"
    );
    assert!(
        actions.contains(&"probe.source.delete".to_owned()),
        "probe.source.delete should be audited: {actions:?}"
    );
}

/// AUDIT-003: Audit entries for source.create have correct target_type and target_id.
#[tokio::test]
async fn source_create_audit_has_correct_target() {
    let app = TestApp::new().await;
    let (router, cookie) = setup_and_login(&app).await;

    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/sources",
            r#"{"name":"target-test","source_type":"auto","url":"https://example.com/sub","auto_update":true,"update_interval_secs":1800,"keep_on_fail":true}"#,
            &cookie,
        ))
        .await
        .expect("create source");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let source_id = json["source"]["id"].as_str().expect("source id").to_owned();

    // Query audit logs filtered by target_type=source.
    let response = router
        .clone()
        .oneshot(get_with_cookie(
            "/api/v1/audit-logs?target_type=source&action=source.create",
            &cookie,
        ))
        .await
        .expect("audit query");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let entries = json["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1, "exactly one source.create entry");
    let entry = &entries[0];
    assert_eq!(entry["action"], "source.create");
    assert_eq!(entry["target_type"], "source");
    assert_eq!(entry["target_id"], source_id);
}
