//! Probe delivery surfaces, separated by source/run/latency responsibility.
mod error;
mod latency_routes;
mod mappers;
mod run_routes;
mod source_routes;

pub(crate) use mappers::{error_class_to_dto, kind_to_dto, probe_type_to_dto, sync_status_to_dto};

/// Register probe REST routes under the root composition state.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<crate::AppState>,
) -> utoipa_axum::router::OpenApiRouter<crate::AppState> {
    latency_routes::register(run_routes::register(source_routes::register(router)))
}
