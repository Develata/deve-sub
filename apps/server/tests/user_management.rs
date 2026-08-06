#![allow(clippy::expect_used)]

//! Integration tests for user management: RBAC, disable user, force logout.
//!
//! Covers AUTH-007 (disable user revokes sessions), AUTH-008 (regular user
//! gets 403 on admin-only routes), AUTH-010 (force logout revokes sessions).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use deve_sub_application::{DbHealthPort, LoginRateLimiter, SubscriptionFetcher};
use deve_sub_domain::{
    NodePoolRepository, RecoveryCodeRepository, SessionRepository, SourceRepository,
    SourceSnapshotRepository, TotpSecretRepository, UserRepository,
};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{
    SqliteHealthCheck, SqliteNodePoolRepository, SqliteRecoveryCodeRepository,
    SqliteSessionRepository, SqliteSourceRepository, SqliteSourceSnapshotRepository,
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

/// Set up an admin and return the session cookie.
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

/// Create a regular user via the admin API and return the new user's JSON.
async fn create_user(
    router: &axum::Router,
    admin_cookie: &str,
    username: &str,
) -> serde_json::Value {
    let body = format!(r#"{{"username":"{username}","password":"user-pwd!","role":"user"}}"#);
    let response = router
        .clone()
        .oneshot(post_with_cookie("/api/v1/users", &body, admin_cookie))
        .await
        .expect("create user");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    serde_json::from_slice(&body).expect("json")
}

/// Login as a regular user and return the session cookie.
async fn login_as(router: &axum::Router, username: &str) -> String {
    let body = format!(r#"{{"username":"{username}","password":"user-pwd!"}}"#);
    let response = router
        .clone()
        .oneshot(post_json("/api/v1/auth/login", &body))
        .await
        .expect("login");
    assert_eq!(response.status(), StatusCode::OK);
    extract_cookie(&response).expect("cookie")
}

/// AUTH-008: Regular users get 403 on admin-only routes.
#[tokio::test]
async fn regular_user_forbidden_on_admin_routes() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;
    let user_json = create_user(&router, &admin_cookie, "alice").await;
    let user_id = user_json["user"]["id"].as_str().expect("user id");
    let user_cookie = login_as(&router, "alice").await;

    // Regular user cannot list users.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/users", &user_cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Regular user cannot create users.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/users",
            r#"{"username":"bob","password":"bob-pwd!","role":"user"}"#,
            &user_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Regular user cannot disable users.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            &format!("/api/v1/users/{user_id}/disable"),
            "{}",
            &user_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Regular user cannot force logout users.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            &format!("/api/v1/users/{user_id}/force-logout"),
            "{}",
            &user_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Admin can list users.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/users", &admin_cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

/// Unauthenticated requests get 401 on admin-only routes.
#[tokio::test]
async fn unauthenticated_rejected_on_admin_routes() {
    let app = TestApp::new().await;
    let router = app.router();

    let response = router
        .clone()
        .oneshot(get("/api/v1/users"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/users",
            r#"{"username":"bob","password":"bob-pwd!","role":"user"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// AUTH-007: Disabling a user revokes all their sessions.
#[tokio::test]
async fn disable_user_revokes_sessions() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;
    let user_json = create_user(&router, &admin_cookie, "alice").await;
    let user_id = user_json["user"]["id"].as_str().expect("user id");
    let user_cookie = login_as(&router, "alice").await;

    // User can access /me before being disabled.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/auth/me", &user_cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // Admin disables the user.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            &format!("/api/v1/users/{user_id}/disable"),
            "{}",
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // User's session is revoked — /me should return 401.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/auth/me", &user_cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Disabled user cannot log in.
    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/login",
            r#"{"username":"alice","password":"user-pwd!"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// AUTH-010: Force logout revokes all sessions for a user without
/// disabling the account — the user can log in again.
#[tokio::test]
async fn force_logout_revokes_sessions() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;
    let user_json = create_user(&router, &admin_cookie, "alice").await;
    let user_id = user_json["user"]["id"].as_str().expect("user id");
    let user_cookie = login_as(&router, "alice").await;

    // User can access /me before force logout.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/auth/me", &user_cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // Admin force-logouts the user.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            &format!("/api/v1/users/{user_id}/force-logout"),
            "{}",
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // User's session is revoked — /me should return 401.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/auth/me", &user_cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Unlike disable, the user CAN log in again after force logout.
    let user_cookie_new = login_as(&router, "alice").await;

    // New session works.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/auth/me", &user_cookie_new))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

/// Creating a user with an existing username returns 409.
#[tokio::test]
async fn create_user_duplicate_username_conflict() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;
    let _ = create_user(&router, &admin_cookie, "alice").await;

    // Duplicate username.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/users",
            r#"{"username":"alice","password":"other-pwd!","role":"user"}"#,
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["error"], "username_exists");
}

/// List users returns all users with correct pagination.
#[tokio::test]
async fn list_users_pagination() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;
    create_user(&router, &admin_cookie, "alice").await;
    create_user(&router, &admin_cookie, "bob").await;

    // List with limit=2 — should return 2 users and a next_cursor.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/users?limit=2", &admin_cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["users"].as_array().expect("array").len(), 2);
    assert!(json["next_cursor"].as_str().is_some());

    let cursor = json["next_cursor"].as_str().expect("cursor").to_owned();

    // Fetch next page.
    let response = router
        .clone()
        .oneshot(get_with_cookie(
            &format!("/api/v1/users?limit=2&cursor={cursor}"),
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["users"].as_array().expect("array").len(), 1);
    // No more pages.
    assert!(json["next_cursor"].is_null());
}

/// Disabling a non-existent user returns 404.
#[tokio::test]
async fn disable_nonexistent_user_404() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;

    // A valid ULID that doesn't exist in the database.
    let fake_id = ulid::Ulid::new().to_string();
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            &format!("/api/v1/users/{fake_id}/disable"),
            "{}",
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// B1: Empty username is rejected with 400.
#[tokio::test]
async fn create_user_empty_username_rejected() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/users",
            r#"{"username":"","password":"valid-pwd!","role":"user"}"#,
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["error"], "invalid_input");
}

/// B1: Short password is rejected with 400.
#[tokio::test]
async fn create_user_short_password_rejected() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/users",
            r#"{"username":"bob","password":"short","role":"user"}"#,
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["error"], "invalid_input");
}

/// B1: setup_admin with short password is rejected with 400.
#[tokio::test]
async fn setup_admin_short_password_rejected() {
    let app = TestApp::new().await;
    let router = app.router();

    let response = router
        .clone()
        .oneshot(post_json(
            "/api/v1/auth/setup",
            r#"{"username":"admin","password":"short"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["error"], "invalid_input");
}

/// W5: Admin cannot disable their own account.
#[tokio::test]
async fn admin_cannot_disable_self() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;

    // Get the admin's own user ID via /me.
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/auth/me", &admin_cookie))
        .await
        .expect("response");
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let admin_id = json["user"]["id"].as_str().expect("admin id");

    // Attempt to disable self.
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            &format!("/api/v1/users/{admin_id}/disable"),
            "{}",
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["error"], "self_disable");

    // Admin should still be able to access /me (not disabled).
    let response = router
        .clone()
        .oneshot(get_with_cookie("/api/v1/auth/me", &admin_cookie))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

/// W3: force_logout on a nonexistent user returns 404.
#[tokio::test]
async fn force_logout_nonexistent_user_404() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;

    let fake_id = ulid::Ulid::new().to_string();
    let response = router
        .clone()
        .oneshot(post_with_cookie(
            &format!("/api/v1/users/{fake_id}/force-logout"),
            "{}",
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// W3: Malformed ULID returns 400 for disable and force-logout.
#[tokio::test]
async fn malformed_ulid_returns_400() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/users/not-a-ulid/disable",
            "{}",
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/users/not-a-ulid/force-logout",
            "{}",
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// W3: Malformed cursor returns 400.
#[tokio::test]
async fn malformed_cursor_returns_400() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(get_with_cookie(
            "/api/v1/users?cursor=not-a-ulid",
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Admin can create another admin user.
#[tokio::test]
async fn admin_can_create_admin_user() {
    let app = TestApp::new().await;
    let router = app.router();

    let admin_cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(post_with_cookie(
            "/api/v1/users",
            r#"{"username":"admin2","password":"admin-pwd!","role":"admin"}"#,
            &admin_cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["user"]["role"], "admin");
}
