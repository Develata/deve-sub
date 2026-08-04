//! HTTP server for Deve Sub: Axum routes, middleware, and OpenAPI.
//!
//! This crate is the Delivery layer. API handlers dispatch to application
//! commands/queries but contain no business rules. See
//! `docs/plan/03-architecture.md` and ADR-0004 for the API boundary policy.

#![cfg_attr(test, allow(clippy::expect_used))]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use deve_sub_domain::{SessionRepository, UserRepository};
use sqlx::sqlite::SqlitePool;
use thiserror::Error;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use utoipa_scalar::{Scalar, Servable};

use deve_sub_security::MasterKey;

pub mod auth;
pub mod routes;
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
    pub db: SqlitePool,
    pub master_key: Arc<MasterKey>,
    pub user_repo: Arc<dyn UserRepository>,
    pub session_repo: Arc<dyn SessionRepository>,
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
    let (api_router, openapi) = routes::build_api_router(state);

    Router::new()
        .merge(api_router)
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

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await?;

    Ok(())
}
