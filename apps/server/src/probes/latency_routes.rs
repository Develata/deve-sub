//! Read-only node latency routes.
use super::mappers::record_to_dto;
use crate::{
    auth::{AdminUser, err},
    state::ProbeState,
};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use deve_sub_contract::ListLatencyRecordsResponse;
use deve_sub_kernel::NodeId;

/// Query parameters for `GET /api/v1/nodes/{id}/latency`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ListLatencyQuery {
    #[serde(default = "default_latency_limit")]
    pub limit: u32,
}

fn default_latency_limit() -> u32 {
    50
}

/// `GET /api/v1/nodes/{id}/latency` — list recent latency records for a node.
#[utoipa::path(
    get,
    path = "/api/v1/nodes/{id}/latency",
    security(("cookie_auth" = [])),
    params(
        ("id" = String, Path, description = "Node ULID"),
        ("limit" = Option<u32>, Query, description = "Max records (1-200, default 50)"),
    ),
    responses(
        (status = 200, description = "Latency records", body = ListLatencyRecordsResponse),
        (status = 400, description = "Invalid id", body = deve_sub_contract::ErrorResponse),
        (status = 401, description = "Not authenticated", body = deve_sub_contract::ErrorResponse),
        (status = 403, description = "Not an admin", body = deve_sub_contract::ErrorResponse),
        (status = 500, description = "Internal error", body = deve_sub_contract::ErrorResponse),
    )
)]
async fn list_node_latency(
    State(state): State<ProbeState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Query(q): Query<ListLatencyQuery>,
) -> Result<Json<ListLatencyRecordsResponse>, (StatusCode, Json<deve_sub_contract::ErrorResponse>)>
{
    let node_id = NodeId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "node id is not a valid ULID",
        )
    })?;
    let limit = q.limit.clamp(1, 200);
    let records = state
        .latency_repo
        .list_for_node(node_id, limit)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_node_latency failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to list latency records",
            )
        })?;
    Ok(Json(ListLatencyRecordsResponse {
        records: records.iter().map(record_to_dto).collect(),
    }))
}

pub(super) fn register(
    router: utoipa_axum::router::OpenApiRouter<crate::AppState>,
) -> utoipa_axum::router::OpenApiRouter<crate::AppState> {
    use utoipa_axum::routes;
    router.routes(routes!(list_node_latency))
}
