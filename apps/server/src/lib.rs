//! HTTP server for Deve Sub: Axum routes, middleware, and OpenAPI.
//!
//! This crate is the Delivery layer. API handlers dispatch to application
//! commands/queries but contain no business rules. See
//! `docs/plan/03-architecture.md` and ADR-0004 for the API boundary policy.

#![cfg_attr(test, allow(clippy::expect_used))]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use axum::Router;
use deve_sub_application::{DbHealthPort, GeoIpPort, LoginRateLimiter, SubscriptionFetcher};
use deve_sub_domain::{
    AuditLogRepository, GenerationCacheRepository, LatencyProbe, LatencyRecordRepository,
    NodeOverrideRepository, NodePoolRepository, PoolMetaRepository, ProbeRunRepository,
    ProbeSourceAdapter, ProbeSourceRepository, RecoveryCodeRepository, SessionRepository,
    ShortCodeRepository, SourceRepository, SourceSnapshotRepository, SubscriptionRepository,
    SubscriptionTokenRepository, TempLinkRepository, TemplateRepository, TemplateVersionRepository,
    TotpSecretRepository, TrafficRepository, UserRepository,
};
use thiserror::Error;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use utoipa_scalar::{Scalar, Servable};

use deve_sub_security::MasterKey;

pub mod audit;
pub mod auth;
pub mod csrf;
pub mod dashboard;
pub mod delivery;
pub mod node_overrides;
pub mod nodes;
pub mod probes;
pub mod routes;
pub mod sources;
pub mod subscriptions;
pub mod template_generation;
pub mod templates;
pub mod traffic;
pub mod twofa;
pub mod users;

/// Errors produced by the server.
#[derive(Debug, Error)]
pub enum ServerError {
    /// The server failed to bind or start.
    #[error("server error: {0}")]
    Start(#[from] std::io::Error),
}

/// Application state shared across all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: deve_sub_application::AppConfig,
    pub master_key: Arc<MasterKey>,
    pub audit_log_repo: Arc<dyn AuditLogRepository>,
    pub user_repo: Arc<dyn UserRepository>,
    pub session_repo: Arc<dyn SessionRepository>,
    pub totp_secret_repo: Arc<dyn TotpSecretRepository>,
    pub recovery_code_repo: Arc<dyn RecoveryCodeRepository>,
    pub source_repo: Arc<dyn SourceRepository>,
    pub snapshot_repo: Arc<dyn SourceSnapshotRepository>,
    pub pool_repo: Arc<dyn NodePoolRepository>,
    pub pool_meta_repo: Arc<dyn PoolMetaRepository>,
    pub override_repo: Arc<dyn NodeOverrideRepository>,
    pub template_repo: Arc<dyn TemplateRepository>,
    pub version_repo: Arc<dyn TemplateVersionRepository>,
    pub cache_repo: Arc<dyn GenerationCacheRepository>,
    pub subscription_repo: Arc<dyn SubscriptionRepository>,
    pub subscription_token_repo: Arc<dyn SubscriptionTokenRepository>,
    pub short_code_repo: Arc<dyn ShortCodeRepository>,
    pub temp_link_repo: Arc<dyn TempLinkRepository>,
    pub traffic_repo: Arc<dyn TrafficRepository>,
    pub probe_source_repo: Arc<dyn ProbeSourceRepository>,
    pub probe_run_repo: Arc<dyn ProbeRunRepository>,
    pub latency_repo: Arc<dyn LatencyRecordRepository>,
    pub probe_adapter: Arc<dyn ProbeSourceAdapter>,
    pub tcp_probe: Arc<dyn LatencyProbe>,
    pub quic_probe: Arc<dyn LatencyProbe>,
    pub real_proxy_probe: Arc<dyn LatencyProbe>,
    pub cancelled_flags: Arc<Mutex<HashMap<deve_sub_kernel::ProbeRunId, Arc<AtomicBool>>>>,
    pub fetcher: Arc<dyn SubscriptionFetcher>,
    pub geoip: Arc<dyn GeoIpPort>,
    pub rate_limiter: Arc<dyn LoginRateLimiter>,
    pub db_health: Arc<dyn DbHealthPort>,
}

/// Build the complete Axum router with all routes and middleware.
///
/// Middleware stack (outermost to innermost):
/// 1. `SetRequestIdLayer` — assign `x-request-id` before tracing
/// 2. `TraceLayer` — structured per-request logs
/// 3. `PropagateRequestIdLayer` — copy `x-request-id` to response
/// 4. `CorsLayer` — permissive CORS for development
/// 5. `CompressionLayer` — gzip compression
///
/// CSRF protection (`Origin` header validation) is applied to the API router
/// only, not to the Scalar docs endpoint.
pub fn build_router(state: AppState) -> Router {
    let (api_router, openapi) = routes::build_api_router(state.clone());

    let delivery_router =
        crate::delivery::register_delivery_routes(Router::new()).with_state(state);

    Router::new()
        .merge(api_router.layer(axum::middleware::from_fn(crate::csrf::csrf_guard)))
        .merge(delivery_router)
        .merge(Scalar::with_url("/docs", openapi))
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}

/// Run the HTTP server on the given bind address.
///
/// The caller provides the shutdown future, keeping signal handling in the
/// binary entry point rather than coupling this library to platform-specific
/// signal APIs.
///
/// # Errors
/// Returns [`ServerError`] if the server fails to bind or start.
pub async fn serve(
    router: Router,
    bind: SocketAddr,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), ServerError> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("HTTP server listening on {bind}");

    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await?;

    Ok(())
}
