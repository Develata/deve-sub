//! Route definitions for health endpoints, API skeleton, and OpenAPI docs.
//!
//! Path, method, and status definitions live here in the API crate per
//! ADR-0004. DTOs and `ToSchema` derives live in `deve-sub-contract`.

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use deve_sub_application::{HealthStatus, HealthView};
use deve_sub_contract::{HealthLiveResponse, HealthReadyResponse};
use utoipa::OpenApi;

use crate::AppState;

/// OpenAPI specification for the Deve Sub API.
#[derive(OpenApi)]
#[openapi(
    paths(health_live, health_ready),
    components(schemas(HealthLiveResponse, HealthReadyResponse)),
    info(
        title = "Deve Sub API",
        version = "0.1.0",
        description = "Self-hosted proxy subscription infrastructure manager"
    )
)]
pub struct ApiDoc;

/// Build and return the OpenAPI document.
#[must_use]
pub fn openapi_docs() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

/// Health and API routes.
pub fn health_routes() -> Router<AppState> {
    Router::new()
        .route("/health/live", get(health_live))
        .route("/health/ready", get(health_ready))
}

/// API v1 skeleton routes.
pub fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/version", get(api_version))
        .route("/", get(web_placeholder))
}

/// Serve the web shell placeholder.
async fn web_placeholder(State(state): State<AppState>) -> axum::response::Response {
    if state.config.server.serve_web {
        axum::response::Html(deve_sub_web::PLACEHOLDER_HTML).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

/// Liveness probe — returns 200 if the process is alive.
#[utoipa::path(
    get,
    path = "/health/live",
    responses(
        (status = 200, description = "Service is alive", body = HealthLiveResponse)
    )
)]
async fn health_live(State(state): State<AppState>) -> Json<HealthLiveResponse> {
    let view = HealthView::live(&state.config.product_name, env!("CARGO_PKG_VERSION"));
    Json(HealthLiveResponse {
        status: view.status,
        product_name: view.product_name,
        version: view.version,
    })
}

/// Readiness probe — returns 200 if the service is ready to serve requests.
///
/// Checks database connectivity. Returns 503 with `degraded` status if the
/// database is unreachable.
#[utoipa::path(
    get,
    path = "/health/ready",
    responses(
        (status = 200, description = "Service is ready", body = HealthReadyResponse),
        (status = 503, description = "Service is not ready", body = HealthReadyResponse)
    )
)]
async fn health_ready(State(state): State<AppState>) -> (StatusCode, Json<HealthReadyResponse>) {
    let db_ok = deve_sub_storage_sqlite::check_database(&state.db)
        .await
        .is_ok();

    let status = if db_ok {
        HealthStatus::Healthy
    } else {
        HealthStatus::Degraded
    };

    let view = HealthView::ready(
        status,
        &state.config.product_name,
        env!("CARGO_PKG_VERSION"),
    );
    let code = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        code,
        Json(HealthReadyResponse {
            status: view.status,
            product_name: view.product_name,
            version: view.version,
        }),
    )
}

/// API version endpoint.
async fn api_version(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "product": state.config.product_name,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
