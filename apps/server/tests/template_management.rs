#![allow(clippy::expect_used)]

//! Integration tests for V3 template management endpoints (GEN-001~004).
//!
//! Covers the full CRUD lifecycle, schema validation, version history, and
//! rollback. See `docs/plan/milestones/M5-generator-and-v3-template.md`
//! Slice 1 and `docs/acceptance/matrix.tsv` rows GEN-001 through GEN-004.

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

fn put_json(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(uri)
        .header("content-type", "application/json")
        .body(json_body(body))
        .expect("request")
}

fn patch_json(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("PATCH")
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

trait RequestExt {
    fn with_header(self, key: &str, value: String) -> Self;
}

impl RequestExt for Request<Body> {
    fn with_header(mut self, key: &str, value: String) -> Self {
        use std::str::FromStr;
        let name = axum::http::HeaderName::from_str(key).expect("header name");
        self.headers_mut()
            .insert(name, value.parse().expect("header"));
        self
    }
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
    let body = axum::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("body");
    serde_json::from_slice(&body).expect("json")
}

fn create_body(name: &str, description: &str, spec_yaml: &str) -> String {
    serde_json::json!({
        "name": name,
        "description": description,
        "spec_yaml": spec_yaml,
    })
    .to_string()
}

fn update_body(name: &str, description: &str, spec_yaml: &str) -> String {
    create_body(name, description, spec_yaml)
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

const VALID_SPEC_YAML_V2: &str = concat!(
    "apiVersion: deve-sub.io/v1\n",
    "kind: SubscriptionTemplate\n",
    "\n",
    "metadata:\n",
    "  name: default-mihomo\n",
    "  description: Updated template\n",
    "  version: 2\n",
    "\n",
    "spec:\n",
    "  targetProfiles:\n",
    "    - mihomo\n",
    "    - sing-box\n",
    "  variables: {}\n",
    "  nodeSelector:\n",
    "    mode: dynamic\n",
    "  proxyGroups: []\n",
    "  rules: []\n",
    "  dns: {}\n",
    "  tun: {}\n",
    "  output: {}",
);

/// GEN-001: Admin can create a template, list it, get it, update it, and
/// delete it — the full CRUD round-trip.
#[tokio::test]
async fn gen001_template_crud_roundtrip() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    // Create.
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &create_body("default-mihomo", "Default Mihomo template", VALID_SPEC_YAML),
            ),
            &cookie,
        ))
        .await
        .expect("create");

    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    assert_eq!(json["template"]["name"], "default-mihomo");
    assert_eq!(json["template"]["description"], "Default Mihomo template");
    assert_eq!(json["template"]["active_version"], 1);
    assert_eq!(json["version"]["version"], 1);
    assert_eq!(json["version"]["is_active"], true);
    let template_id = json["template"]["id"].as_str().expect("id").to_owned();
    let version_id = json["version"]["id"]
        .as_str()
        .expect("version id")
        .to_owned();

    // List.
    let response = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/templates"), &cookie))
        .await
        .expect("list");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert_eq!(json["templates"].as_array().expect("array").len(), 1);
    assert_eq!(json["templates"][0]["name"], "default-mihomo");

    // Get.
    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/templates/{template_id}")),
            &cookie,
        ))
        .await
        .expect("get");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert_eq!(json["template"]["id"], template_id);

    // Update — creates version 2.
    let response = router
        .clone()
        .oneshot(with_cookie(
            put_json(
                &format!("/api/v1/templates/{template_id}"),
                &update_body("default-mihomo", "Updated template", VALID_SPEC_YAML_V2),
            ),
            &cookie,
        ))
        .await
        .expect("update");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert_eq!(json["template"]["active_version"], 2);
    assert_eq!(json["version"]["version"], 2);
    assert_eq!(json["version"]["is_active"], true);
    assert_ne!(
        json["version"]["id"].as_str().expect("new version id"),
        version_id
    );

    // Delete.
    let response = router
        .clone()
        .oneshot(with_cookie(
            delete(&format!("/api/v1/templates/{template_id}")),
            &cookie,
        ))
        .await
        .expect("delete");
    assert_eq!(response.status(), StatusCode::OK);

    // Verify gone.
    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/templates/{template_id}")),
            &cookie,
        ))
        .await
        .expect("get after delete");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// GEN-002: Invalid templates return field-level errors with 400, and no
/// partial template is persisted.
#[tokio::test]
async fn gen002_invalid_template_rejected() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    // Wrong apiVersion.
    let bad_yaml = VALID_SPEC_YAML.replace("deve-sub.io/v1", "v2");
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &create_body("bad-version", "", &bad_yaml),
            ),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Empty name in metadata.
    let bad_yaml = VALID_SPEC_YAML.replace("default-mihomo", "");
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &create_body("bad-empty-meta-name", "", &bad_yaml),
            ),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Forbidden script tag.
    let bad_yaml =
        format!("{VALID_SPEC_YAML}\n  script: \"require('child_process').exec('rm -rf /')\"");
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &create_body("bad-script", "", &bad_yaml),
            ),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Malformed YAML.
    let bad_yaml = "apiVersion: deve-sub.io/v1\nkind: SubscriptionTemplate\n  metadata: [invalid";
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/templates", &create_body("bad-yaml", "", bad_yaml)),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Empty top-level name (application boundary).
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/templates", &create_body("", "", VALID_SPEC_YAML)),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Verify no templates were persisted.
    let response = router
        .clone()
        .oneshot(with_cookie(get("/api/v1/templates"), &cookie))
        .await
        .expect("list");
    let json = body_to_json(response).await;
    assert_eq!(json["templates"].as_array().expect("array").len(), 0);
}

/// GEN-003: Editing a template creates a new version; the version history
/// preserves all prior versions.
#[tokio::test]
async fn gen003_edit_creates_new_version() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    // Create — version 1.
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &create_body("versioned-template", "v1", VALID_SPEC_YAML),
            ),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    let template_id = json["template"]["id"].as_str().expect("id").to_owned();
    let v1_id = json["version"]["id"].as_str().expect("v1 id").to_owned();

    // Update — version 2.
    let response = router
        .clone()
        .oneshot(with_cookie(
            put_json(
                &format!("/api/v1/templates/{template_id}"),
                &update_body("versioned-template", "v2", VALID_SPEC_YAML_V2),
            ),
            &cookie,
        ))
        .await
        .expect("update");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert_eq!(json["version"]["version"], 2);
    let v2_id = json["version"]["id"].as_str().expect("v2 id").to_owned();

    // List versions — should have 2, newest first.
    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/templates/{template_id}/versions")),
            &cookie,
        ))
        .await
        .expect("versions");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    let versions = json["versions"].as_array().expect("array");
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0]["version"], 2);
    assert_eq!(versions[0]["id"], v2_id);
    assert_eq!(versions[0]["is_active"], true);
    assert_eq!(versions[1]["version"], 1);
    assert_eq!(versions[1]["id"], v1_id);
    assert_eq!(versions[1]["is_active"], false);
}

/// GEN-004: Rollback restores a prior version as active; the version history
/// is preserved (no versions deleted).
#[tokio::test]
async fn gen004_rollback_restores_prior_version() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    // Create — version 1.
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &create_body("rollback-template", "v1", VALID_SPEC_YAML),
            ),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    let template_id = json["template"]["id"].as_str().expect("id").to_owned();
    let v1_id = json["version"]["id"].as_str().expect("v1 id").to_owned();

    // Update — version 2.
    let response = router
        .clone()
        .oneshot(with_cookie(
            put_json(
                &format!("/api/v1/templates/{template_id}"),
                &update_body("rollback-template", "v2", VALID_SPEC_YAML_V2),
            ),
            &cookie,
        ))
        .await
        .expect("update");
    assert_eq!(response.status(), StatusCode::OK);

    // Rollback to version 1.
    let rollback_body = format!(r#"{{"version_id":"{v1_id}"}}"#);
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                &format!("/api/v1/templates/{template_id}/rollback"),
                &rollback_body,
            ),
            &cookie,
        ))
        .await
        .expect("rollback");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert_eq!(json["version"]["id"], v1_id);
    assert_eq!(json["version"]["version"], 1);
    assert_eq!(json["version"]["is_active"], true);

    // Verify the template aggregate reflects the rollback.
    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/templates/{template_id}")),
            &cookie,
        ))
        .await
        .expect("get after rollback");
    let json = body_to_json(response).await;
    assert_eq!(json["template"]["active_version"], 1);

    // Verify version history still has both versions (no deletion).
    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/templates/{template_id}/versions")),
            &cookie,
        ))
        .await
        .expect("versions after rollback");
    let json = body_to_json(response).await;
    let versions = json["versions"].as_array().expect("array");
    assert_eq!(versions.len(), 2, "rollback must not delete any version");

    // The v1 entry should now be active; v2 inactive.
    let v1_entry = versions
        .iter()
        .find(|v| v["id"] == v1_id)
        .expect("v1 in history");
    let v2_entry = versions
        .iter()
        .find(|v| v["version"] == 2)
        .expect("v2 in history");
    assert_eq!(v1_entry["is_active"], true);
    assert_eq!(v2_entry["is_active"], false);
}

/// Unauthenticated requests are rejected with 401.
#[tokio::test]
async fn unauthenticated_rejected() {
    let app = TestApp::new().await;
    let router = app.router();

    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/templates",
            &create_body("test", "", VALID_SPEC_YAML),
        ))
        .await
        .expect("create");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// Duplicate name returns 409.
#[tokio::test]
async fn duplicate_name_returns_conflict() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let r1 = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &create_body("dup-name", "", VALID_SPEC_YAML),
            ),
            &cookie,
        ))
        .await
        .expect("first");
    assert_eq!(r1.status(), StatusCode::CREATED);

    let r2 = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &create_body("dup-name", "", VALID_SPEC_YAML),
            ),
            &cookie,
        ))
        .await
        .expect("second");
    assert_eq!(r2.status(), StatusCode::CONFLICT);
}

// ---------------------------------------------------------------------------
// GEN-005 ~ GEN-009, GEN-011: Node selection, quick-group, sort, deletion
// ---------------------------------------------------------------------------

/// Import nodes via the API and return their IDs from the response.
///
/// WHY: `ImportNodesResponse` returns per-line `outcomes` with
/// `{"status": "inserted"|"duplicate", "data": "<id>"}` rather than a
/// `nodes` array. We collect IDs from both `inserted` and `duplicate`
/// outcomes so callers can reference them regardless of dedup.
async fn import_nodes(router: &axum::Router, cookie: &str, content: &str) -> Vec<String> {
    let body = serde_json::json!({
        "content": content,
        "source_type": "uri_list",
    })
    .to_string();
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/nodes/import", &body),
            cookie,
        ))
        .await
        .expect("import");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    json["outcomes"]
        .as_array()
        .expect("outcomes array")
        .iter()
        .filter_map(|o| {
            let status = o["status"].as_str().expect("status");
            if status == "inserted" || status == "duplicate" {
                Some(o["data"].as_str().expect("id").to_owned())
            } else {
                None
            }
        })
        .collect()
}

/// Set a node's region via the API.
async fn set_region(router: &axum::Router, cookie: &str, node_id: &str, region: &str) {
    let body = serde_json::json!({ "region": region }).to_string();
    let response = router
        .clone()
        .oneshot(with_cookie(
            patch_json(&format!("/api/v1/nodes/{node_id}/region"), &body),
            cookie,
        ))
        .await
        .expect("set region");
    assert_eq!(response.status(), StatusCode::OK);
}

/// Disable a node via the override API (sets `enabled: false`).
async fn disable_node(router: &axum::Router, cookie: &str, node_id: &str) {
    let body = serde_json::json!({ "enabled": false }).to_string();
    let response = router
        .clone()
        .oneshot(with_cookie(
            patch_json(&format!("/api/v1/nodes/{node_id}/override"), &body),
            cookie,
        ))
        .await
        .expect("disable node");
    assert_eq!(response.status(), StatusCode::OK);
}

/// Resolve a template and return the JSON response.
async fn resolve_template_api(
    router: &axum::Router,
    cookie: &str,
    template_id: &str,
) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/templates/{template_id}/resolve")),
            cookie,
        ))
        .await
        .expect("resolve");
    assert_eq!(response.status(), StatusCode::OK);
    body_to_json(response).await
}

/// GEN-005: Dynamic selection — new nodes automatically enter the result.
#[tokio::test]
async fn gen005_dynamic_selection_includes_new_nodes() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    import_nodes(
        &router,
        &cookie,
        "trojan://pw@host-a.example.com:443#NodeA\ntrojan://pw@host-b.example.com:443#NodeB",
    )
    .await;

    let yaml = concat!(
        "apiVersion: deve-sub.io/v1\n",
        "kind: SubscriptionTemplate\n",
        "\n",
        "metadata:\n",
        "  name: gen005-dynamic\n",
        "  description: Dynamic selection test\n",
        "  version: 1\n",
        "\n",
        "spec:\n",
        "  targetProfiles:\n",
        "    - mihomo\n",
        "  variables: {}\n",
        "  nodeSelector:\n",
        "    mode: dynamic\n",
        "    filters:\n",
        "      - field: protocol\n",
        "        value: trojan\n",
        "  proxyGroups: []\n",
        "  rules: []\n",
        "  dns: {}\n",
        "  tun: {}\n",
        "  output: {}",
    );
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/templates", &create_body("gen005", "test", yaml)),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    let template_id = json["template"]["id"].as_str().expect("id").to_owned();

    let res1 = resolve_template_api(&router, &cookie, &template_id).await;
    assert_eq!(res1["selected_node_ids"].as_array().expect("ids").len(), 2);

    import_nodes(&router, &cookie, "trojan://pw@host-c.example.com:443#NodeC").await;

    let res2 = resolve_template_api(&router, &cookie, &template_id).await;
    assert_eq!(
        res2["selected_node_ids"].as_array().expect("ids").len(),
        3,
        "dynamic selection should include newly imported nodes"
    );
}

/// GEN-006: Fixed snapshot — new nodes do not auto-join.
#[tokio::test]
async fn gen006_fixed_selection_excludes_new_nodes() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let ids = import_nodes(
        &router,
        &cookie,
        "trojan://pw@host-a.example.com:443#NodeA\ntrojan://pw@host-b.example.com:443#NodeB",
    )
    .await;
    assert_eq!(ids.len(), 2);
    let id_a = &ids[0];
    let id_b = &ids[1];

    let yaml = format!(
        concat!(
            "apiVersion: deve-sub.io/v1\n",
            "kind: SubscriptionTemplate\n",
            "\n",
            "metadata:\n",
            "  name: gen006-fixed\n",
            "  description: Fixed selection test\n",
            "  version: 1\n",
            "\n",
            "spec:\n",
            "  targetProfiles:\n",
            "    - mihomo\n",
            "  variables: {{}}\n",
            "  nodeSelector:\n",
            "    mode: fixed\n",
            "    filters: []\n",
            "    nodeIds:\n",
            "      - {a}\n",
            "      - {b}\n",
            "    nodeRevision: 1\n",
            "  proxyGroups: []\n",
            "  rules: []\n",
            "  dns: {{}}\n",
            "  tun: {{}}\n",
            "  output: {{}}",
        ),
        a = id_a,
        b = id_b,
    );

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/templates", &create_body("gen006", "test", &yaml)),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    let template_id = json["template"]["id"].as_str().expect("id").to_owned();

    let res1 = resolve_template_api(&router, &cookie, &template_id).await;
    assert_eq!(res1["selected_node_ids"].as_array().expect("ids").len(), 2,);

    import_nodes(&router, &cookie, "trojan://pw@host-c.example.com:443#NodeC").await;

    let res2 = resolve_template_api(&router, &cookie, &template_id).await;
    assert_eq!(
        res2["selected_node_ids"].as_array().expect("ids").len(),
        2,
        "fixed selection should not include newly imported nodes"
    );
}

/// GEN-007: Quick-group by region — group members correct.
#[tokio::test]
async fn gen007_quick_group_by_region() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let ids = import_nodes(
        &router,
        &cookie,
        "trojan://pw@host-a.example.com:443#NodeA\ntrojan://pw@host-b.example.com:443#NodeB\ntrojan://pw@host-c.example.com:443#NodeC",
    )
    .await;
    set_region(&router, &cookie, &ids[0], "US").await;
    set_region(&router, &cookie, &ids[1], "US").await;
    set_region(&router, &cookie, &ids[2], "JP").await;

    let yaml = concat!(
        "apiVersion: deve-sub.io/v1\n",
        "kind: SubscriptionTemplate\n",
        "\n",
        "metadata:\n",
        "  name: gen007-region\n",
        "  description: Region group test\n",
        "  version: 1\n",
        "\n",
        "spec:\n",
        "  targetProfiles:\n",
        "    - mihomo\n",
        "  variables: {}\n",
        "  nodeSelector:\n",
        "    mode: dynamic\n",
        "  proxyGroups:\n",
        "    - name: us-nodes\n",
        "      type: select\n",
        "      members: []\n",
        "      filter:\n",
        "        region: US\n",
        "  rules: []\n",
        "  dns: {}\n",
        "  tun: {}\n",
        "  output: {}",
    );
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/templates", &create_body("gen007", "test", yaml)),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    let template_id = json["template"]["id"].as_str().expect("id").to_owned();

    let res = resolve_template_api(&router, &cookie, &template_id).await;
    let groups = res["groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 1);
    let us_group = &groups[0];
    assert_eq!(us_group["group_name"], "us-nodes");
    let quick_ids = us_group["quick_group_node_ids"]
        .as_array()
        .expect("quick ids");
    assert_eq!(quick_ids.len(), 2, "should match 2 US nodes");
    assert!(us_group["missing"].as_array().expect("missing").is_empty());
}

/// GEN-008: Quick-group by protocol — group members correct.
#[tokio::test]
async fn gen008_quick_group_by_protocol() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    import_nodes(
        &router,
        &cookie,
        "trojan://pw@host-a.example.com:443#NodeA\ntrojan://pw@host-b.example.com:443#NodeB\nvless://uuid@host-c.example.com:443?type=tcp&encryption=none#NodeC",
    )
    .await;

    let yaml = concat!(
        "apiVersion: deve-sub.io/v1\n",
        "kind: SubscriptionTemplate\n",
        "\n",
        "metadata:\n",
        "  name: gen008-protocol\n",
        "  description: Protocol group test\n",
        "  version: 1\n",
        "\n",
        "spec:\n",
        "  targetProfiles:\n",
        "    - mihomo\n",
        "  variables: {}\n",
        "  nodeSelector:\n",
        "    mode: dynamic\n",
        "  proxyGroups:\n",
        "    - name: trojan-nodes\n",
        "      type: select\n",
        "      members: []\n",
        "      filter:\n",
        "        protocol: trojan\n",
        "  rules: []\n",
        "  dns: {}\n",
        "  tun: {}\n",
        "  output: {}",
    );
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/templates", &create_body("gen008", "test", yaml)),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    let template_id = json["template"]["id"].as_str().expect("id").to_owned();

    let res = resolve_template_api(&router, &cookie, &template_id).await;
    let groups = res["groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 1);
    let trojan_group = &groups[0];
    assert_eq!(trojan_group["group_name"], "trojan-nodes");
    let quick_ids = trojan_group["quick_group_node_ids"]
        .as_array()
        .expect("quick ids");
    assert_eq!(quick_ids.len(), 2, "should match 2 trojan nodes");
}

/// GEN-009: Drag sort — member order persists across save and reload.
#[tokio::test]
async fn gen009_drag_sort_persists_order() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let ids = import_nodes(
        &router,
        &cookie,
        "trojan://pw@host-a.example.com:443#NodeA\ntrojan://pw@host-b.example.com:443#NodeB\ntrojan://pw@host-c.example.com:443#NodeC",
    )
    .await;
    let id_a = &ids[0];
    let id_b = &ids[1];
    let id_c = &ids[2];

    let yaml_original = format!(
        concat!(
            "apiVersion: deve-sub.io/v1\n",
            "kind: SubscriptionTemplate\n",
            "\n",
            "metadata:\n",
            "  name: gen009-sort\n",
            "  description: Drag sort test\n",
            "  version: 1\n",
            "\n",
            "spec:\n",
            "  targetProfiles:\n",
            "    - mihomo\n",
            "  variables: {{}}\n",
            "  nodeSelector:\n",
            "    mode: dynamic\n",
            "  proxyGroups:\n",
            "    - name: ordered\n",
            "      type: select\n",
            "      members:\n",
            "        - kind: node\n",
            "          id: {a}\n",
            "        - kind: node\n",
            "          id: {b}\n",
            "        - kind: node\n",
            "          id: {c}\n",
            "  rules: []\n",
            "  dns: {{}}\n",
            "  tun: {{}}\n",
            "  output: {{}}",
        ),
        a = id_a,
        b = id_b,
        c = id_c,
    );

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &create_body("gen009", "test", &yaml_original),
            ),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    let template_id = json["template"]["id"].as_str().expect("id").to_owned();
    let version_id = json["version"]["id"].as_str().expect("vid").to_owned();

    let versions_response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/templates/{template_id}/versions")),
            &cookie,
        ))
        .await
        .expect("versions");
    let versions_json = body_to_json(versions_response).await;
    let versions = versions_json["versions"].as_array().expect("versions");
    let active_version = versions
        .iter()
        .find(|v| v["id"] == version_id)
        .expect("active version");
    let spec_yaml = active_version["spec_yaml"].as_str().expect("yaml");

    let pos_a = spec_yaml.find(id_a).expect("id_a in yaml");
    let pos_b = spec_yaml.find(id_b).expect("id_b in yaml");
    let pos_c = spec_yaml.find(id_c).expect("id_c in yaml");
    assert!(
        pos_a < pos_b && pos_b < pos_c,
        "initial order should be A, B, C"
    );

    // Update with reordered members: C, A, B.
    let yaml_reordered = format!(
        concat!(
            "apiVersion: deve-sub.io/v1\n",
            "kind: SubscriptionTemplate\n",
            "\n",
            "metadata:\n",
            "  name: gen009-sort\n",
            "  description: Drag sort test\n",
            "  version: 2\n",
            "\n",
            "spec:\n",
            "  targetProfiles:\n",
            "    - mihomo\n",
            "  variables: {{}}\n",
            "  nodeSelector:\n",
            "    mode: dynamic\n",
            "  proxyGroups:\n",
            "    - name: ordered\n",
            "      type: select\n",
            "      members:\n",
            "        - kind: node\n",
            "          id: {c}\n",
            "        - kind: node\n",
            "          id: {a}\n",
            "        - kind: node\n",
            "          id: {b}\n",
            "  rules: []\n",
            "  dns: {{}}\n",
            "  tun: {{}}\n",
            "  output: {{}}",
        ),
        c = id_c,
        a = id_a,
        b = id_b,
    );

    let update_response = router
        .clone()
        .oneshot(with_cookie(
            put_json(
                &format!("/api/v1/templates/{template_id}"),
                &update_body("gen009", "Drag sort test", &yaml_reordered),
            ),
            &cookie,
        ))
        .await
        .expect("update");
    assert_eq!(update_response.status(), StatusCode::OK);
    let update_json = body_to_json(update_response).await;
    let new_version_id = update_json["version"]["id"]
        .as_str()
        .expect("new vid")
        .to_owned();

    // Verify new order.
    let versions_response2 = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/templates/{template_id}/versions")),
            &cookie,
        ))
        .await
        .expect("versions");
    let versions_json2 = body_to_json(versions_response2).await;
    let versions2 = versions_json2["versions"].as_array().expect("versions");
    let new_version = versions2
        .iter()
        .find(|v| v["id"] == new_version_id)
        .expect("new version");
    let new_spec_yaml = new_version["spec_yaml"].as_str().expect("yaml");

    let new_pos_a = new_spec_yaml.find(id_a).expect("id_a in yaml");
    let new_pos_b = new_spec_yaml.find(id_b).expect("id_b in yaml");
    let new_pos_c = new_spec_yaml.find(id_c).expect("id_c in yaml");
    assert!(
        new_pos_c < new_pos_a && new_pos_a < new_pos_b,
        "reordered should be C, A, B"
    );
}

/// GEN-011: Node deletion — related group references are reported.
#[tokio::test]
async fn gen011_node_deletion_reports_missing_refs() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let ids = import_nodes(
        &router,
        &cookie,
        "trojan://pw@host-a.example.com:443#NodeA\ntrojan://pw@host-b.example.com:443#NodeB",
    )
    .await;
    let id_a = &ids[0];
    let id_b = &ids[1];

    let yaml = format!(
        concat!(
            "apiVersion: deve-sub.io/v1\n",
            "kind: SubscriptionTemplate\n",
            "\n",
            "metadata:\n",
            "  name: gen011-deletion\n",
            "  description: Node deletion test\n",
            "  version: 1\n",
            "\n",
            "spec:\n",
            "  targetProfiles:\n",
            "    - mihomo\n",
            "  variables: {{}}\n",
            "  nodeSelector:\n",
            "    mode: dynamic\n",
            "  proxyGroups:\n",
            "    - name: test\n",
            "      type: select\n",
            "      members:\n",
            "        - kind: node\n",
            "          id: {a}\n",
            "        - kind: node\n",
            "          id: {b}\n",
            "  rules: []\n",
            "  dns: {{}}\n",
            "  tun: {{}}\n",
            "  output: {{}}",
        ),
        a = id_a,
        b = id_b,
    );

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/templates", &create_body("gen011", "test", &yaml)),
            &cookie,
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response).await;
    let template_id = json["template"]["id"].as_str().expect("id").to_owned();

    // Both nodes active → no missing refs.
    let res1 = resolve_template_api(&router, &cookie, &template_id).await;
    let groups1 = res1["groups"].as_array().expect("groups");
    assert_eq!(
        groups1[0]["explicit_node_ids"]
            .as_array()
            .expect("ids")
            .len(),
        2
    );
    assert!(
        groups1[0]["missing"]
            .as_array()
            .expect("missing")
            .is_empty()
    );

    // Disable node A → should be reported as inactive.
    disable_node(&router, &cookie, id_a).await;

    let res2 = resolve_template_api(&router, &cookie, &template_id).await;
    let groups2 = res2["groups"].as_array().expect("groups");
    let missing = groups2[0]["missing"].as_array().expect("missing");
    assert_eq!(missing.len(), 1, "one node should be reported as missing");
    assert_eq!(missing[0]["node_id"].as_str().expect("node_id"), id_a);
    assert_eq!(missing[0]["reason"].as_str().expect("reason"), "inactive");
    let explicit = groups2[0]["explicit_node_ids"]
        .as_array()
        .expect("explicit");
    assert_eq!(explicit.len(), 1, "only node B should be active");
}
