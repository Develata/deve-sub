#![allow(clippy::expect_used)]

//! Integration tests for M10 Slice 3: traffic daily snapshots and history API.
//!
//! TRAFFIC-001: the daily aggregation job sums traffic records per
//! subscription per UTC day and upserts `traffic_daily_snapshots` rows.
//! TRAFFIC-002: `GET /api/v1/dashboard/traffic/history` returns continuous
//! daily data with gap-filling.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use time::OffsetDateTime;
use tower::ServiceExt;

use deve_sub_application::{
    DbHealthPort, GeoIpPort, LoginRateLimiter, SubscriptionFetcher, aggregate_daily_traffic,
};
use deve_sub_domain::{
    AuditLogRepository, GenerationCacheRepository, LatencyProbe, LatencyRecordRepository,
    NodeOverrideRepository, NodePoolRepository, PoolMetaRepository, ProbeRunRepository,
    ProbeSourceRepository, RecoveryCodeRepository, SessionRepository, ShortCodeRepository,
    SourceRepository, SourceSnapshotRepository, SubscriptionRepository,
    SubscriptionTokenRepository, TempLinkRepository, TemplateRepository, TemplateVersionRepository,
    TotpSecretRepository, TrafficDailySnapshotRepository, TrafficRecord, TrafficRepository,
    TrafficSourceKind, UserRepository,
};
use deve_sub_kernel::{SubscriptionId, Timestamp, TrafficRecordId};
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

fn with_cookie(mut req: Request<Body>, cookie: &str) -> Request<Body> {
    req.headers_mut()
        .insert("cookie", cookie.parse().expect("cookie header"));
    req
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
    let json = body_to_json(res).await;
    json["template"]["id"]
        .as_str()
        .expect("template id")
        .to_owned()
}

async fn create_sub(router: &axum::Router, cookie: &str, template_id: &str, slug: &str) -> String {
    let body = serde_json::json!({
        "name": slug,
        "slug": slug,
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
    let json = body_to_json(res).await;
    json["subscription"]["id"]
        .as_str()
        .expect("sub id")
        .to_owned()
}

fn make_record(
    subscription_id: SubscriptionId,
    source_kind: TrafficSourceKind,
    upload: u64,
    download: u64,
    recorded_at: Timestamp,
    source_ref: &str,
) -> TrafficRecord {
    TrafficRecord {
        id: TrafficRecordId::new(),
        subscription_id,
        source_kind,
        upload,
        download,
        recorded_at,
        source_ref: source_ref.to_owned(),
    }
}

fn ts_at(year: i32, month: u8, day: u8, hour: u8) -> Timestamp {
    let dt = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(year, time::Month::try_from(month).expect("month"), day)
            .expect("date"),
        time::Time::from_hms(hour, 0, 0).expect("time"),
    );
    Timestamp::from_offset_date_time(dt)
}

fn iso_at(year: i32, month: u8, day: u8) -> String {
    let dt = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(year, time::Month::try_from(month).expect("month"), day)
            .expect("date"),
        time::Time::from_hms(0, 0, 0).expect("time"),
    );
    dt.format(&time::format_description::well_known::Rfc3339)
        .expect("iso")
}

/// TRAFFIC-001: aggregation sums per-day traffic per subscription and
/// upserts `traffic_daily_snapshots` with correct totals and source breakdown.
#[tokio::test]
async fn traffic001_daily_snapshot_aggregation() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;
    let sub_id_str = create_sub(&router, &cookie, &template_id, "sub-001").await;
    let sub_id = SubscriptionId::parse(&sub_id_str).expect("sub id");

    // Insert traffic records on 2025-06-10 (two AirportHeader + one Probe),
    // plus one record on 2025-06-11 that must NOT be counted in the 06-10 day.
    let day = "2025-06-10";
    let day_start = iso_at(2025, 6, 10);
    let day_end = iso_at(2025, 6, 11);

    let records = [
        make_record(
            sub_id,
            TrafficSourceKind::AirportHeader,
            1_000,
            2_000,
            ts_at(2025, 6, 10, 3),
            "https://upstream.example/sub",
        ),
        make_record(
            sub_id,
            TrafficSourceKind::AirportHeader,
            500,
            700,
            ts_at(2025, 6, 10, 15),
            "https://upstream.example/sub",
        ),
        make_record(
            sub_id,
            TrafficSourceKind::Probe,
            300,
            400,
            ts_at(2025, 6, 10, 20),
            "nezha:server-1",
        ),
        make_record(
            sub_id,
            TrafficSourceKind::AirportHeader,
            9_999,
            9_999,
            ts_at(2025, 6, 11, 1),
            "https://upstream.example/sub",
        ),
    ];
    for record in &records {
        app.state
            .traffic_repo
            .create(record)
            .await
            .expect("create traffic record");
    }

    let count = aggregate_daily_traffic(
        app.state.traffic_repo.as_ref(),
        app.state.traffic_daily_snapshot_repo.as_ref(),
        day,
        &day_start,
        &day_end,
    )
    .await
    .expect("aggregate");
    assert_eq!(count, 1, "one subscription had traffic on that day");

    let snapshots = app
        .state
        .traffic_daily_snapshot_repo
        .list_for_subscription(sub_id, day, day)
        .await
        .expect("list snapshots");
    assert_eq!(snapshots.len(), 1, "one snapshot row for the day");
    let snap = &snapshots[0];
    assert_eq!(snap.date, day);
    assert_eq!(snap.total_upload, 1_800, "1000 + 500 + 300");
    assert_eq!(snap.total_download, 3_100, "2000 + 700 + 400");
    let mut by_kind: std::collections::BTreeMap<&str, (u64, u64)> = snap
        .source_breakdown
        .iter()
        .map(|(k, u, d)| (k.as_db_char(), (*u, *d)))
        .collect();
    let airport = by_kind.remove("A").expect("airport breakdown");
    assert_eq!(airport, (1_500, 2_700), "1000+500 up, 2000+700 down");
    let probe = by_kind.remove("P").expect("probe breakdown");
    assert_eq!(probe, (300, 400));
    assert!(by_kind.is_empty(), "no other source kinds");
}

/// TRAFFIC-001: idempotency — re-running aggregation for the same day
/// replaces (not appends) the snapshot.
#[tokio::test]
async fn traffic001_aggregation_is_idempotent() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;
    let sub_id_str = create_sub(&router, &cookie, &template_id, "sub-idem").await;
    let sub_id = SubscriptionId::parse(&sub_id_str).expect("sub id");

    let day = "2025-06-10";
    let day_start = iso_at(2025, 6, 10);
    let day_end = iso_at(2025, 6, 11);

    let record = make_record(
        sub_id,
        TrafficSourceKind::AirportHeader,
        1_000,
        2_000,
        ts_at(2025, 6, 10, 5),
        "https://upstream.example/sub",
    );
    app.state
        .traffic_repo
        .create(&record)
        .await
        .expect("create");

    for _ in 0..3 {
        aggregate_daily_traffic(
            app.state.traffic_repo.as_ref(),
            app.state.traffic_daily_snapshot_repo.as_ref(),
            day,
            &day_start,
            &day_end,
        )
        .await
        .expect("aggregate");
    }

    let snapshots = app
        .state
        .traffic_daily_snapshot_repo
        .list_for_subscription(sub_id, day, day)
        .await
        .expect("list");
    assert_eq!(snapshots.len(), 1, "upsert keeps a single row");
    assert_eq!(snapshots[0].total_upload, 1_000);
    assert_eq!(snapshots[0].total_download, 2_000);
}

/// TRAFFIC-002: history API returns continuous daily data with gap-filling.
#[tokio::test]
async fn traffic002_history_api_continuous_with_gaps() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;
    let sub_id_str = create_sub(&router, &cookie, &template_id, "sub-hist").await;
    let sub_id = SubscriptionId::parse(&sub_id_str).expect("sub id");

    // Populate two non-adjacent days of snapshots directly via the repository.
    let snap_a = deve_sub_domain::TrafficDailySnapshot::new(
        sub_id,
        "2025-06-08".to_owned(),
        100,
        200,
        vec![(TrafficSourceKind::AirportHeader, 100, 200)],
    );
    let snap_c = deve_sub_domain::TrafficDailySnapshot::new(
        sub_id,
        "2025-06-10".to_owned(),
        300,
        400,
        vec![(TrafficSourceKind::Probe, 300, 400)],
    );
    app.state
        .traffic_daily_snapshot_repo
        .upsert(&snap_a)
        .await
        .expect("upsert a");
    app.state
        .traffic_daily_snapshot_repo
        .upsert(&snap_c)
        .await
        .expect("upsert c");

    let uri = format!("/api/v1/dashboard/traffic/history?subscription_id={sub_id_str}&days=3");
    // days=3 from "today" would not cover 2025-06-08..10, so we rely on the
    // snapshot range itself: the API fills the inclusive range
    // [start_date, end_date] returned by the snapshots. We query with a wide
    // enough window by requesting the snapshots' date span directly.
    // Since the API computes the range from `days` ending today, we instead
    // verify the gap-fill logic at the application layer.
    let points = deve_sub_application::list_traffic_history_for_subscription(
        app.state.traffic_daily_snapshot_repo.as_ref(),
        sub_id,
        "2025-06-08",
        "2025-06-10",
    )
    .await
    .expect("history");
    assert_eq!(points.len(), 3, "three days in range, gap filled");
    assert_eq!(points[0].date, "2025-06-08");
    assert_eq!(points[0].total_upload, 100);
    assert_eq!(points[0].total_download, 200);
    assert_eq!(points[1].date, "2025-06-09");
    assert_eq!(points[1].total_upload, 0, "gap day filled with zero");
    assert_eq!(points[1].total_download, 0);
    assert_eq!(points[2].date, "2025-06-10");
    assert_eq!(points[2].total_upload, 300);
    assert_eq!(points[2].total_download, 400);

    // Verify the HTTP API surface returns a 200 with the right shape.
    let _ = uri;
    let response = router
        .clone()
        .oneshot(get_with_cookie(
            &format!("/api/v1/dashboard/traffic/history?subscription_id={sub_id_str}&days=1"),
            &cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response).await;
    assert!(json["points"].is_array(), "points is an array");
    assert!(
        json["scoped_to_subscription"]
            .as_bool()
            .expect("scoped flag"),
        "scoped_to_subscription true when subscription_id given"
    );
}

/// TRAFFIC-002: global history (no subscription_id) aggregates across all
/// subscriptions per day.
#[tokio::test]
async fn traffic002_history_api_global_aggregation() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let template_id = create_template(&router, &cookie).await;
    let sub_a = create_sub(&router, &cookie, &template_id, "sub-ga").await;
    let sub_b = create_sub(&router, &cookie, &template_id, "sub-gb").await;
    let id_a = SubscriptionId::parse(&sub_a).expect("id a");
    let id_b = SubscriptionId::parse(&sub_b).expect("id b");

    let snap_a = deve_sub_domain::TrafficDailySnapshot::new(
        id_a,
        "2025-06-09".to_owned(),
        100,
        200,
        vec![(TrafficSourceKind::AirportHeader, 100, 200)],
    );
    let snap_b = deve_sub_domain::TrafficDailySnapshot::new(
        id_b,
        "2025-06-09".to_owned(),
        50,
        60,
        vec![(TrafficSourceKind::Probe, 50, 60)],
    );
    app.state
        .traffic_daily_snapshot_repo
        .upsert(&snap_a)
        .await
        .expect("upsert a");
    app.state
        .traffic_daily_snapshot_repo
        .upsert(&snap_b)
        .await
        .expect("upsert b");

    let points = deve_sub_application::list_traffic_history_global(
        app.state.traffic_daily_snapshot_repo.as_ref(),
        "2025-06-09",
        "2025-06-09",
    )
    .await
    .expect("global history");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].date, "2025-06-09");
    assert_eq!(points[0].total_upload, 150, "100 + 50 across subscriptions");
    assert_eq!(points[0].total_download, 260, "200 + 60");

    let mut kinds: std::collections::BTreeMap<&str, (u64, u64)> = points[0]
        .source_breakdown
        .iter()
        .map(|(k, u, d)| (k.as_db_char(), (*u, *d)))
        .collect();
    assert_eq!(kinds.remove("A").expect("airport"), (100, 200));
    assert_eq!(kinds.remove("P").expect("probe"), (50, 60));
    assert!(kinds.is_empty());
}

/// TRAFFIC-002: history API rejects an invalid subscription_id with 400.
#[tokio::test]
async fn traffic002_history_api_invalid_subscription_id() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;

    let response = router
        .clone()
        .oneshot(get_with_cookie(
            "/api/v1/dashboard/traffic/history?subscription_id=not-a-ulid&days=7",
            &cookie,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// TRAFFIC-002: history API requires admin authentication.
#[tokio::test]
async fn traffic002_history_api_requires_auth() {
    let app = TestApp::new().await;
    let router = app.router();

    let response = router
        .clone()
        .oneshot(get("/api/v1/dashboard/traffic/history?days=7"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
