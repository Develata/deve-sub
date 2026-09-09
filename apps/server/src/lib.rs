//! HTTP server for Deve Sub: Axum routes, middleware, and OpenAPI.
//!
//! This crate is the Delivery layer. API handlers dispatch to application
//! commands/queries but contain no business rules. See
//! `docs/plan/03-architecture.md` and ADR-0004 for the API boundary policy.

// WHY: test harnesses use unwrap/expect for infallible fixtures (Uri/Request
// builders, temp dirs). Denying them in test code adds noise without catching
// real bugs. Non-test code remains deny-by-default per workspace lints.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use deve_sub_application::{
    DbHealthPort, GeoIpPort, JobSupervisor, LoginRateLimiter, SubscriptionFetcher,
};
use deve_sub_domain::{
    AuditLogRepository, GenerationCacheRepository, LatencyProbe, LatencyRecordRepository,
    NodeOverrideRepository, NodePoolRepository, PoolMetaRepository, ProbeRunRepository,
    ProbeSourceAdapter, ProbeSourceRepository, RecoveryCodeRepository, SessionRepository,
    ShortCodeRepository, SourceRefreshJobRepository, SourceRepository, SourceSnapshotRepository,
    SubscriptionRepository, SubscriptionTokenRepository, TempLinkRepository, TemplateRepository,
    TemplateVersionRepository, TotpSecretRepository, TrafficDailySnapshotRepository,
    TrafficRepository, UserRepository,
};
use thiserror::Error;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::services::{ServeDir, ServeFile};
use utoipa_scalar::{Scalar, Servable};

use deve_sub_security::MasterKey;

pub mod audit;
pub mod auth;
pub mod csrf;
pub mod dashboard;
pub mod delivery;
pub mod logging;
pub mod node_overrides;
pub mod nodes;
pub mod probes;
pub mod routes;
pub mod source_refresh;
pub mod sources;
pub mod state;
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
    /// Active HTTP connections did not drain within the shutdown grace.
    #[error("HTTP shutdown exceeded its grace period")]
    ShutdownTimeout,
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
    pub refresh_job_repo: Arc<dyn SourceRefreshJobRepository>,
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
    pub traffic_daily_snapshot_repo: Arc<dyn TrafficDailySnapshotRepository>,
    pub probe_source_repo: Arc<dyn ProbeSourceRepository>,
    pub probe_run_repo: Arc<dyn ProbeRunRepository>,
    pub latency_repo: Arc<dyn LatencyRecordRepository>,
    pub probe_adapter: Arc<dyn ProbeSourceAdapter>,
    pub tcp_probe: Arc<dyn LatencyProbe>,
    pub quic_probe: Arc<dyn LatencyProbe>,
    pub real_proxy_probe: Arc<dyn LatencyProbe>,
    pub cancelled_flags: Arc<Mutex<HashMap<deve_sub_kernel::ProbeRunId, Arc<AtomicBool>>>>,
    pub refresh_cancel_flags:
        Arc<Mutex<HashMap<deve_sub_kernel::SourceRefreshJobId, Arc<AtomicBool>>>>,
    pub job_supervisor: Arc<JobSupervisor>,
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
/// 4. `CorsLayer` — only when `server.allowed_origins` is non-empty (the
///    default same-origin deployment needs no CORS headers)
/// 5. `CompressionLayer` — gzip compression
///
/// CSRF protection (`Origin` header validation) is applied to the API router
/// only, not to the Scalar docs endpoint.
pub fn build_router(state: AppState) -> Router {
    let (api_router, openapi) = routes::build_api_router(state.clone());

    let delivery_router =
        crate::delivery::register_delivery_routes(Router::new()).with_state(state.clone());

    let dist_path = std::path::PathBuf::from(&state.config.server.web_dist_dir);
    let serve_web = state.config.server.serve_web;
    let dist_exists = dist_path.exists();

    let router = Router::new()
        .merge(api_router.layer(axum::middleware::from_fn(crate::csrf::csrf_guard)))
        .merge(delivery_router)
        .merge(Scalar::with_url("/docs", openapi));

    let router = if serve_web && dist_exists {
        let serve_dir =
            ServeDir::new(&dist_path).fallback(ServeFile::new(dist_path.join("index.html")));
        router.fallback_service(serve_dir)
    } else if serve_web {
        router.fallback(|| async {
            axum::response::Html(deve_sub_web::PLACEHOLDER_HTML).into_response()
        })
    } else {
        router.fallback(|| async { StatusCode::NOT_FOUND.into_response() })
    };

    let router = router.layer(CompressionLayer::new());
    let router = match cors_layer(&state.config.server.allowed_origins) {
        Some(cors) => router.layer(cors),
        None => router,
    };
    router
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(crate::logging::redacting_trace_layer())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}

/// Build a restrictive CORS layer from the configured origin allowlist.
///
/// WHY: `CorsLayer::permissive()` allowed every origin by default, which is
/// the wrong production posture even with cookie-auth guarded by CSRF Origin
/// validation — non-credentialed cross-origin reads were unconstrained. The
/// web UI is same-origin by default, so an empty list emits NO CORS layer
/// (same-origin requests do not use CORS); a non-empty list emits exactly
/// those origins. Invalid entries are skipped with a warning rather than
/// aborting startup.
fn cors_layer(allowed_origins: &[String]) -> Option<CorsLayer> {
    let mut origins = Vec::with_capacity(allowed_origins.len());
    for raw in allowed_origins {
        match raw.parse::<axum::http::HeaderValue>() {
            Ok(value) => origins.push(value),
            Err(e) => tracing::warn!(origin = raw, error = %e, "invalid allowed_origin skipped"),
        }
    }
    if origins.is_empty() {
        return None;
    }
    Some(CorsLayer::new().allow_origin(origins))
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

    serve_listener(
        router,
        listener,
        shutdown,
        std::time::Duration::from_secs(30),
    )
    .await
}

async fn serve_listener(
    router: Router,
    listener: tokio::net::TcpListener,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    grace: std::time::Duration,
) -> Result<(), ServerError> {
    use std::future::IntoFuture;
    let (draining_tx, draining_rx) = tokio::sync::oneshot::channel();
    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.await;
        let _ = draining_tx.send(());
    })
    .into_future();
    tokio::pin!(server);
    tokio::select! {
        biased;
        result = &mut server => result?,
        _ = draining_rx => {
            // A client holding an incomplete request must not keep the process
            // alive indefinitely. The composition root still drains workers
            // and closes storage after this error before runtime/process exit.
            tokio::time::timeout(grace, &mut server)
                .await.map_err(|_| ServerError::ShutdownTimeout)??;
        }
    }
    Ok(())
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn stalled_http_handler_cannot_hold_shutdown_forever() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let entered = Arc::new(tokio::sync::Notify::new());
        let notification = entered.clone();
        let router = Router::new().route(
            "/stall",
            axum::routing::get(move || async move {
                notification.notify_one();
                std::future::pending::<()>().await;
                StatusCode::OK
            }),
        );
        let (tx, rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve_listener(
            router,
            listener,
            async {
                let _ = rx.await;
            },
            std::time::Duration::from_millis(20),
        ));
        let mut client = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect");
        client
            .write_all(b"GET /stall HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .expect("request");
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .expect("handler entered");
        tx.send(()).expect("signal");
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), server)
            .await
            .expect("bounded drain")
            .expect("join");
        assert!(matches!(result, Err(ServerError::ShutdownTimeout)));
    }
}
