//! HTTP server for Deve Sub: Axum routes, middleware, and OpenAPI.
//!
//! This crate is the Delivery layer. API handlers dispatch to application
//! commands/queries but contain no business rules. See
//! `docs/plan/03-architecture.md` and ADR-0004 for the API boundary policy.

#![cfg_attr(test, allow(clippy::expect_used))]

use std::net::SocketAddr;

use axum::Router;
use sqlx::sqlite::SqlitePool;
use thiserror::Error;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use utoipa_scalar::{Scalar, Servable};

pub mod routes;

/// Errors produced by the server.
#[derive(Debug, Error)]
pub enum ServerError {
    /// The server failed to bind or start.
    #[error("server error: {0}")]
    Start(String),
}

/// Application state shared across all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: deve_sub_application::AppConfig,
    pub db: SqlitePool,
}

/// Build the complete Axum router with all routes and middleware.
///
/// Middleware stack (outermost to innermost):
/// 1. `SetRequestIdLayer` — assign `x-request-id` before tracing
/// 2. `TraceLayer` — structured per-request logs
/// 3. `PropagateRequestIdLayer` — copy `x-request-id` to response
/// 4. `CorsLayer` — permissive CORS for development
/// 5. `CompressionLayer` — gzip compression
pub fn build_router(state: AppState) -> Router {
    let openapi = routes::openapi_docs();

    Router::new()
        .merge(routes::health_routes())
        .merge(routes::api_routes())
        .merge(Scalar::with_url("/docs", openapi))
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .with_state(state)
}

/// Run the HTTP server on the given bind address.
///
/// # Errors
/// Returns [`ServerError`] if the server fails to bind or start.
pub async fn serve(
    router: Router,
    bind: SocketAddr,
    shutdown: tokio::signal::unix::SignalKind,
) -> Result<(), ServerError> {
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| ServerError::Start(e.to_string()))?;

    tracing::info!("HTTP server listening on {bind}");

    let shutdown_signal = async move {
        // SAFETY: Installing a SIGTERM handler on Unix is infallible in
        // practice — the only failure mode is an invalid signal number,
        // which SIGTERM is not. If it somehow fails, the server will not
        // shut down gracefully but will still function.
        #[allow(
            clippy::expect_used,
            reason = "SIGTERM handler installation is infallible on Unix"
        )]
        let mut sig = tokio::signal::unix::signal(shutdown)
            .expect("installing SIGTERM handler is infallible on Unix");
        sig.recv().await;
        tracing::info!("shutdown signal received, draining connections");
    };

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .map_err(|e| ServerError::Start(e.to_string()))
}
