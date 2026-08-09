//! Route definitions for health endpoints, API skeleton, and OpenAPI docs.
//!
//! Path, method, and status definitions live here in the API crate per
//! ADR-0004. DTOs and `ToSchema` derives live in `deve-sub-contract`.
//! Routes and OpenAPI paths are registered simultaneously via
//! `OpenApiRouter` to prevent spec/code drift.

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use deve_sub_application::{HealthStatus, HealthView};
use deve_sub_contract::{HealthLiveResponse, HealthReadyResponse};
use utoipa::openapi::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;

/// Build the OpenAPI document with info from the application config.
///
/// The product name and version are injected at runtime to avoid hardcoded
/// scattering (AGENTS.md §"Naming"). A cookie-based security scheme is
/// declared so that protected endpoints can annotate their security
/// requirement (OA1).
#[must_use]
pub fn build_openapi(product_name: &str) -> OpenApi {
    use utoipa::openapi::InfoBuilder;
    use utoipa::openapi::schema::ComponentsBuilder;
    use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};

    OpenApi::builder()
        .info(
            InfoBuilder::new()
                .title(format!("{product_name} API"))
                .version(env!("CARGO_PKG_VERSION"))
                .description(Some(
                    "Self-hosted proxy subscription infrastructure manager",
                ))
                .build(),
        )
        .components(Some(
            ComponentsBuilder::new()
                .security_scheme(
                    "cookie_auth",
                    SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("deve_sub_session"))),
                )
                .build(),
        ))
        .build()
}

/// Register all `#[utoipa::path]`-annotated API routes on the given router.
///
/// This is the single registration point shared by both the live server
/// ([`build_api_router`]) and the spec exporter ([`build_openapi_spec`]),
/// ensuring the exported spec and the served routes cannot drift apart.
fn register_api_routes(router: OpenApiRouter<AppState>) -> OpenApiRouter<AppState> {
    let router = router
        .routes(routes!(health_live))
        .routes(routes!(health_ready));
    let router = crate::auth::register(router);
    let router = crate::twofa::register(router);
    let router = crate::users::register(router);
    let router = crate::sources::register(router);
    let router = crate::node_overrides::register(router);
    let router = crate::templates::register(router);
    let router = crate::template_generation::register(router);
    let router = crate::subscriptions::register(router);
    let router = crate::traffic::register(router);
    let router = crate::probes::register(router);
    crate::nodes::register(router)
}

/// Build the complete OpenAPI document with all registered paths.
///
/// Unlike [`build_api_router`], this does not require a database pool —
/// it registers the routes via `OpenApiRouter` solely to collect the
/// OpenAPI paths, then discards the Axum router. Used by the `openapi`
/// CLI subcommand for spec export.
#[must_use]
pub fn build_openapi_spec(product_name: &str) -> OpenApi {
    let openapi = build_openapi(product_name);

    register_api_routes(OpenApiRouter::with_openapi(openapi)).into_openapi()
}

/// Build the complete router with OpenAPI documentation.
///
/// Routes registered via `routes!` are simultaneously added to the Axum
/// router and the OpenAPI spec, ensuring they cannot drift apart.
/// Non-API routes (web placeholder) are added via plain `Router::route`
/// and are intentionally excluded from the spec.
pub fn build_api_router(state: AppState) -> (Router, OpenApi) {
    let openapi = build_openapi(&state.config.product_name);

    let (router, openapi) = register_api_routes(OpenApiRouter::with_openapi(openapi))
        .route("/", get(web_placeholder))
        .with_state(state)
        .split_for_parts();

    (router, openapi)
}

/// Serve the web shell placeholder.
///
/// Returns 404 when `serve_web` is disabled (headless mode).
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
    let db_ok = state.db_health.check().await.is_ok();

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
