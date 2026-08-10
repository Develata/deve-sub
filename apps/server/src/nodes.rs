//! Node pool route handlers (admin-only).
//!
//! Implements the `/api/v1/nodes/*` endpoints: list, get, and manual
//! import. All routes require an authenticated admin via the [`AdminUser`]
//! extractor. See `docs/plan/milestones/M4-sources-and-node-pool.md` Slice 3.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use deve_sub_application::source::{self, ListNodesParams};
use deve_sub_contract::{
    ErrorResponse, ImportNodesRequest, ImportNodesResponse, ImportOutcomeDto, ListNodesResponse,
    NodeDto, NodeResponse, RegionMethodDto, TagDto,
};
use deve_sub_domain::{ImportOutcome, NodePoolEntry, RegionMethod, Tag};
use deve_sub_kernel::NodeId;

use crate::AppState;
use crate::auth::{AdminUser, err, ts_to_iso8601};
use crate::sources::source_type_from_dto;

/// Query parameters for `GET /api/v1/nodes` (cursor pagination + filters).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ListNodesQuery {
    /// Maximum number of nodes to return (1-100, default 20).
    #[serde(default = "default_page_size")]
    pub limit: u32,
    /// Pagination cursor — the ULID of the last node from the previous page.
    pub cursor: Option<String>,
    /// Filter by protocol kind (e.g. `Trojan`, `Vless`).
    pub protocol: Option<String>,
    /// Filter by region (case-sensitive exact match).
    pub region: Option<String>,
    /// Include nodes marked missing from their source.
    #[serde(default)]
    pub include_missing: bool,
    /// Include inactive (disabled) nodes.
    #[serde(default)]
    pub include_inactive: bool,
}

fn default_page_size() -> u32 {
    20
}

/// Convert a domain [`NodePoolEntry`] to the DTO representation.
fn node_to_dto(entry: &NodePoolEntry) -> NodeDto {
    NodeDto {
        id: entry.node.id.to_string(),
        display_name: entry.node.display_name.clone(),
        protocol: entry.node.protocol.to_string(),
        host: entry.node.endpoint.host.uri_host(),
        port: entry.node.endpoint.port,
        region: entry.node.region.value.clone(),
        region_method: match entry.node.region.method {
            RegionMethod::Auto => RegionMethodDto::Auto,
            RegionMethod::Manual => RegionMethodDto::Manual,
        },
        source_label: entry.node.source.source_label.clone(),
        imported_at: ts_to_iso8601(entry.node.source.imported_at),
        is_active: entry.is_active,
        missing_from_source: entry.missing_from_source,
        revision: entry.revision,
        created_at: ts_to_iso8601(entry.created_at),
        tags: entry.tags.iter().map(tag_to_dto).collect(),
        chain: entry
            .node
            .chain
            .as_ref()
            .map(|c| c.nodes.iter().map(|n| n.to_string()).collect())
            .unwrap_or_default(),
    }
}

/// Convert a domain [`Tag`] to the DTO representation.
pub(crate) fn tag_to_dto(tag: &Tag) -> TagDto {
    TagDto {
        id: tag.id.to_string(),
        name: tag.name.clone(),
        color: tag.color.clone(),
    }
}

/// `GET /api/v1/nodes` — list nodes with filters and cursor pagination
/// (admin only).
#[utoipa::path(
    get,
    path = "/api/v1/nodes",
    security(("cookie_auth" = [])),
    params(
        ("limit" = Option<u32>, Query, description = "Max nodes per page (1-100, default 20)"),
        ("cursor" = Option<String>, Query, description = "Pagination cursor (last node ULID)"),
        ("protocol" = Option<String>, Query, description = "Filter by protocol kind"),
        ("region" = Option<String>, Query, description = "Filter by region"),
        ("include_missing" = Option<bool>, Query, description = "Include missing-from-source nodes"),
        ("include_inactive" = Option<bool>, Query, description = "Include inactive nodes"),
    ),
    responses(
        (status = 200, description = "Node list", body = ListNodesResponse),
        (status = 400, description = "Invalid cursor or protocol", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn list_nodes(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<ListNodesQuery>,
) -> Result<Json<ListNodesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = q.limit.clamp(1, 100);

    let cursor = q
        .cursor
        .as_deref()
        .map(NodeId::parse)
        .transpose()
        .map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "cursor is not a valid ULID",
            )
        })?;

    let protocol = match q.protocol.as_deref() {
        Some(s) => Some(
            parse_protocol_query(s)
                .map_err(|msg| err(StatusCode::BAD_REQUEST, "invalid_protocol", msg))?,
        ),
        None => None,
    };

    let params = ListNodesParams {
        protocol,
        region: q.region,
        include_missing: q.include_missing,
        include_inactive: q.include_inactive,
        cursor,
        limit,
    };

    let entries = source::list_nodes(state.pool_repo.as_ref(), params)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_nodes failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to list nodes",
            )
        })?;

    let next_cursor = if entries.len() as u32 >= limit {
        entries.last().map(|e| e.node.id.to_string())
    } else {
        None
    };

    let node_dtos: Vec<NodeDto> = entries.iter().map(node_to_dto).collect();
    Ok(Json(ListNodesResponse {
        nodes: node_dtos,
        next_cursor,
    }))
}

/// `GET /api/v1/nodes/{id}` — get a node by ID (admin only).
#[utoipa::path(
    get,
    path = "/api/v1/nodes/{id}",
    security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Node ULID")),
    responses(
        (status = 200, description = "Node found", body = NodeResponse),
        (status = 400, description = "Invalid node id", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 404, description = "Node not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn get_node(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<NodeResponse>, (StatusCode, Json<ErrorResponse>)> {
    let node_id = NodeId::parse(&id).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "invalid_id",
            "node id is not a valid ULID",
        )
    })?;

    let entry = source::get_node(state.pool_repo.as_ref(), node_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "get_node failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to get node",
            )
        })?
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "node_not_found",
                "node does not exist",
            )
        })?;

    Ok(Json(NodeResponse {
        node: node_to_dto(&entry),
    }))
}

/// `POST /api/v1/nodes/import` — manually import a batch of nodes (admin
/// only). NODE-001 / NODE-002 / NODE-003.
///
/// Parses the request body content with the same protocol parsers used for
/// source refresh and imports the resulting nodes into the pool with
/// dedup by `(protocol_kind, host, port)`. Duplicates are counted but not
/// overwritten.
#[utoipa::path(
    post,
    path = "/api/v1/nodes/import",
    security(("cookie_auth" = [])),
    request_body = ImportNodesRequest,
    responses(
        (status = 200, description = "Import completed", body = ImportNodesResponse),
        (status = 400, description = "Invalid input or unparseable content", body = ErrorResponse),
        (status = 401, description = "Not authenticated", body = ErrorResponse),
        (status = 403, description = "Not an admin", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
async fn import_nodes(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(req): Json<ImportNodesRequest>,
) -> Result<Json<ImportNodesResponse>, (StatusCode, Json<ErrorResponse>)> {
    if req.content.is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_input",
            "content must not be empty",
        ));
    }

    let source_type = source_type_from_dto(req.source_type);
    let parsed =
        source::parse_for_import(source_type, None, req.content.as_bytes()).map_err(|e| {
            tracing::warn!(error = %e, "import parse failed");
            err(
                StatusCode::BAD_REQUEST,
                "parse_failed",
                "failed to parse import content",
            )
        })?;

    let failed_count = u64::try_from(parsed.failed.len()).unwrap_or(u64::MAX);
    let result = source::import_nodes(state.pool_repo.as_ref(), parsed.nodes)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "import_nodes failed");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to import nodes",
            )
        })?;

    let outcomes = result
        .outcomes
        .into_iter()
        .map(|o| match o {
            ImportOutcome::Inserted(id) => ImportOutcomeDto::Inserted(id.to_string()),
            ImportOutcome::Duplicate(id) => ImportOutcomeDto::Duplicate(id.to_string()),
            ImportOutcome::Failed(raw) => ImportOutcomeDto::Failed(raw),
        })
        .collect();

    // WHY: `result.failed` is always 0 because `import_nodes` receives only
    // pre-parsed nodes; failed lines are tracked in `parsed.failed` instead.
    // We add them here so the response reports the true failure count.
    let total_failed = result.failed + failed_count;

    Ok(Json(ImportNodesResponse {
        new_nodes: result.new_nodes,
        duplicate_nodes: result.duplicate_nodes,
        failed: total_failed,
        outcomes,
    }))
}

/// Parse a protocol kind from the query string.
///
/// WHY: `ProtocolKind` uses `PascalCase` serde names but `Display` uses a
/// humanized form (e.g. `TUIC v5`). The query parameter accepts the serde
/// name for stability with the JSON API.
fn parse_protocol_query(s: &str) -> Result<deve_sub_domain::ProtocolKind, &'static str> {
    let json = format!("\"{s}\"");
    serde_json::from_str(&json).map_err(|_| "unknown protocol kind")
}

/// Register all node pool routes on the given `OpenApiRouter`.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(list_nodes))
        .routes(routes!(get_node))
        .routes(routes!(import_nodes))
}
