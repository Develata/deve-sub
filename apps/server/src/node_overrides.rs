//! Node override, tag, and region route handlers (admin-only).
//!
//! Implements `/api/v1/nodes/{id}/{override,region,tags}`, `/batch-enabled`,
//! `/batch-tags`, and `/api/v1/tags`. See M4 NODE-004/005/006/010.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use deve_sub_application::source::{self, UpdateOverrideParams};
use deve_sub_contract::{
    BatchEnabledRequest, BatchResultDto, BatchTagsRequest, CreateTagRequest, ErrorResponse,
    ListTagsResponse, NodeChainResponse, NodeOverrideDto, NodeOverrideResponse, RegionMethodDto,
    RegionResponse, SetNodeChainRequest, SetNodeTagsRequest, SetRegionRequest, TagResponse,
    UpdateOverrideRequest,
};
use deve_sub_domain::{NodeChainError, NodeOverride, RegionAssignment, RegionMethod, SourceError};
use deve_sub_kernel::{NodeId, TagId};

use crate::AppState;
use crate::auth::{AdminUser, err};
use crate::nodes::tag_to_dto;

const BAD_REQUEST: StatusCode = StatusCode::BAD_REQUEST;
const CONFLICT: StatusCode = StatusCode::CONFLICT;
const INTERNAL_SERVER_ERROR: StatusCode = StatusCode::INTERNAL_SERVER_ERROR;
const NOT_FOUND: StatusCode = StatusCode::NOT_FOUND;

/// `PATCH /api/v1/nodes/{id}/override` — create or replace a node's manual
/// override (NODE-010, admin only).
#[utoipa::path(patch, path = "/api/v1/nodes/{id}/override", security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Node ULID")), request_body = UpdateOverrideRequest,
    responses((status = 200, description = "Override applied", body = NodeOverrideResponse), (status = 400, description = "Invalid node id", body = ErrorResponse), (status = 401, description = "Not authenticated", body = ErrorResponse), (status = 403, description = "Not an admin", body = ErrorResponse), (status = 404, description = "Node not found", body = ErrorResponse), (status = 500, description = "Internal error", body = ErrorResponse)))]
async fn update_override(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateOverrideRequest>,
) -> Result<Json<NodeOverrideResponse>, (StatusCode, Json<ErrorResponse>)> {
    let node_id = parse_node_id(&id)?;
    let params = UpdateOverrideParams {
        display_name: req.display_name,
        region: req.region,
        enabled: req.enabled,
        sni: req.sni,
        skip_cert_verify: req.skip_cert_verify,
        fingerprint: req.fingerprint,
        sort_order: req.sort_order,
    };
    let ov = source::update_override(
        state.override_repo.as_ref(),
        state.pool_repo.as_ref(),
        node_id,
        params,
    )
    .await
    .map_err(map_error)?;
    Ok(Json(NodeOverrideResponse {
        override_: override_to_dto(&ov),
    }))
}

/// `DELETE /api/v1/nodes/{id}/override` — delete a node's override (admin only).
#[utoipa::path(delete, path = "/api/v1/nodes/{id}/override", security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Node ULID")),
    responses((status = 204, description = "Override deleted"), (status = 400, description = "Invalid node id", body = ErrorResponse), (status = 401, description = "Not authenticated", body = ErrorResponse), (status = 403, description = "Not an admin", body = ErrorResponse), (status = 500, description = "Internal error", body = ErrorResponse)))]
async fn delete_override(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let node_id = parse_node_id(&id)?;
    source::delete_override(state.override_repo.as_ref(), node_id)
        .await
        .map_err(map_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `PATCH /api/v1/nodes/{id}/region` — set or clear a manual region (NODE-006).
#[utoipa::path(patch, path = "/api/v1/nodes/{id}/region", security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Node ULID")), request_body = SetRegionRequest,
    responses((status = 200, description = "Region updated", body = RegionResponse), (status = 400, description = "Invalid node id", body = ErrorResponse), (status = 401, description = "Not authenticated", body = ErrorResponse), (status = 403, description = "Not an admin", body = ErrorResponse), (status = 404, description = "Node not found", body = ErrorResponse), (status = 500, description = "Internal error", body = ErrorResponse)))]
async fn set_region(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<SetRegionRequest>,
) -> Result<Json<RegionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let node_id = parse_node_id(&id)?;
    let assignment = source::set_manual_region(
        state.override_repo.as_ref(),
        state.pool_repo.as_ref(),
        node_id,
        req.region,
    )
    .await
    .map_err(map_error)?;
    Ok(Json(region_to_response(&assignment)))
}

/// `PUT /api/v1/nodes/{id}/chain` — set or clear a node's proxy chain
/// (NODE-017 / NODE-018, admin only).
#[utoipa::path(put, path = "/api/v1/nodes/{id}/chain", security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Node ULID")), request_body = SetNodeChainRequest,
    responses((status = 200, description = "Chain updated", body = NodeChainResponse), (status = 400, description = "Invalid node id or chain structure", body = ErrorResponse), (status = 401, description = "Not authenticated", body = ErrorResponse), (status = 403, description = "Not an admin", body = ErrorResponse), (status = 404, description = "Node not found", body = ErrorResponse), (status = 409, description = "Chain would create a cycle", body = ErrorResponse), (status = 500, description = "Internal error", body = ErrorResponse)))]
async fn set_node_chain(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<SetNodeChainRequest>,
) -> Result<Json<NodeChainResponse>, (StatusCode, Json<ErrorResponse>)> {
    let node_id = parse_node_id(&id)?;
    let chain_nodes: Option<Vec<NodeId>> = if req.nodes.is_empty() {
        None
    } else {
        Some(parse_ids(&req.nodes)?)
    };

    let result = source::set_node_chain(state.pool_repo.as_ref(), node_id, chain_nodes)
        .await
        .map_err(map_error)?;
    Ok(Json(NodeChainResponse {
        nodes: result
            .unwrap_or_default()
            .into_iter()
            .map(|n| n.to_string())
            .collect(),
    }))
}

/// `POST /api/v1/nodes/batch-enabled` — batch set enabled flag (NODE-004).
#[utoipa::path(post, path = "/api/v1/nodes/batch-enabled", security(("cookie_auth" = [])),
    request_body = BatchEnabledRequest,
    responses((status = 200, description = "Batch applied", body = BatchResultDto), (status = 400, description = "Invalid node ids", body = ErrorResponse), (status = 401, description = "Not authenticated", body = ErrorResponse), (status = 403, description = "Not an admin", body = ErrorResponse), (status = 500, description = "Internal error", body = ErrorResponse)))]
async fn batch_set_enabled(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(req): Json<BatchEnabledRequest>,
) -> Result<Json<BatchResultDto>, (StatusCode, Json<ErrorResponse>)> {
    let node_ids: Vec<NodeId> = parse_ids(&req.node_ids)?;
    let updated = source::batch_set_enabled(state.override_repo.as_ref(), node_ids, req.enabled)
        .await
        .map_err(map_error)?;
    Ok(Json(BatchResultDto { updated }))
}

/// `PUT /api/v1/nodes/{id}/tags` — replace tags for a single node (NODE-005).
#[utoipa::path(put, path = "/api/v1/nodes/{id}/tags", security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Node ULID")), request_body = SetNodeTagsRequest,
    responses((status = 204, description = "Tags updated"), (status = 400, description = "Invalid ids", body = ErrorResponse), (status = 401, description = "Not authenticated", body = ErrorResponse), (status = 403, description = "Not an admin", body = ErrorResponse), (status = 500, description = "Internal error", body = ErrorResponse)))]
async fn set_node_tags(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<SetNodeTagsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let node_id = parse_node_id(&id)?;
    let tag_ids: Vec<TagId> = parse_ids(&req.tag_ids)?;
    source::set_node_tags(state.override_repo.as_ref(), node_id, tag_ids)
        .await
        .map_err(map_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/nodes/batch-tags` — batch replace tags (NODE-005).
#[utoipa::path(post, path = "/api/v1/nodes/batch-tags", security(("cookie_auth" = [])),
    request_body = BatchTagsRequest,
    responses((status = 204, description = "Batch tags applied"), (status = 400, description = "Invalid ids", body = ErrorResponse), (status = 401, description = "Not authenticated", body = ErrorResponse), (status = 403, description = "Not an admin", body = ErrorResponse), (status = 500, description = "Internal error", body = ErrorResponse)))]
async fn batch_set_tags(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(req): Json<BatchTagsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let mut assignments = Vec::with_capacity(req.assignments.len());
    for a in &req.assignments {
        let node_id = parse_node_id(&a.node_id)?;
        let tag_ids: Vec<TagId> = parse_ids(&a.tag_ids)?;
        assignments.push((node_id, tag_ids));
    }
    source::batch_set_tags(state.override_repo.as_ref(), assignments)
        .await
        .map_err(map_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/tags` — list all tags (admin only).
#[utoipa::path(get, path = "/api/v1/tags", security(("cookie_auth" = [])),
    responses((status = 200, description = "Tag list", body = ListTagsResponse), (status = 401, description = "Not authenticated", body = ErrorResponse), (status = 403, description = "Not an admin", body = ErrorResponse), (status = 500, description = "Internal error", body = ErrorResponse)))]
async fn list_tags(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<ListTagsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let tags = source::list_tags(state.override_repo.as_ref())
        .await
        .map_err(map_error)?;
    Ok(Json(ListTagsResponse {
        tags: tags.iter().map(tag_to_dto).collect(),
    }))
}

/// `POST /api/v1/tags` — create a new tag (admin only).
#[utoipa::path(post, path = "/api/v1/tags", security(("cookie_auth" = [])),
    request_body = CreateTagRequest,
    responses((status = 201, description = "Tag created", body = TagResponse), (status = 400, description = "Invalid input", body = ErrorResponse), (status = 401, description = "Not authenticated", body = ErrorResponse), (status = 403, description = "Not an admin", body = ErrorResponse), (status = 409, description = "Tag name already exists", body = ErrorResponse), (status = 500, description = "Internal error", body = ErrorResponse)))]
async fn create_tag(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(req): Json<CreateTagRequest>,
) -> Result<(StatusCode, Json<TagResponse>), (StatusCode, Json<ErrorResponse>)> {
    let tag = source::create_tag(
        state.override_repo.as_ref(),
        &req.name,
        req.color.as_deref(),
    )
    .await
    .map_err(map_error)?;
    Ok((
        StatusCode::CREATED,
        Json(TagResponse {
            tag: tag_to_dto(&tag),
        }),
    ))
}

/// `DELETE /api/v1/tags/{id}` — delete a tag by ID (admin only).
#[utoipa::path(delete, path = "/api/v1/tags/{id}", security(("cookie_auth" = [])),
    params(("id" = String, Path, description = "Tag ULID")),
    responses((status = 204, description = "Tag deleted"), (status = 400, description = "Invalid tag id", body = ErrorResponse), (status = 401, description = "Not authenticated", body = ErrorResponse), (status = 403, description = "Not an admin", body = ErrorResponse), (status = 404, description = "Tag not found", body = ErrorResponse), (status = 500, description = "Internal error", body = ErrorResponse)))]
async fn delete_tag(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let tag_id = id
        .parse::<TagId>()
        .map_err(|_| err(BAD_REQUEST, "invalid_id", "tag id is not a valid ULID"))?;
    source::delete_tag(state.override_repo.as_ref(), tag_id)
        .await
        .map_err(map_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_node_id(s: &str) -> Result<NodeId, (StatusCode, Json<ErrorResponse>)> {
    s.parse::<NodeId>()
        .map_err(|_| err(BAD_REQUEST, "invalid_id", "node id is not a valid ULID"))
}

fn parse_ids<T: std::str::FromStr>(
    ids: &[String],
) -> Result<Vec<T>, (StatusCode, Json<ErrorResponse>)> {
    ids.iter()
        .map(|s| {
            s.parse::<T>()
                .map_err(|_| err(BAD_REQUEST, "invalid_id", "one or more invalid ULIDs"))
        })
        .collect()
}

fn override_to_dto(ov: &NodeOverride) -> NodeOverrideDto {
    NodeOverrideDto {
        display_name: ov.display_name.clone(),
        region: ov.region.clone(),
        enabled: ov.enabled,
        sni: ov.sni.clone(),
        skip_cert_verify: ov.skip_cert_verify,
        fingerprint: ov.fingerprint.clone(),
        sort_order: ov.sort_order,
    }
}

fn region_to_response(assignment: &RegionAssignment) -> RegionResponse {
    RegionResponse {
        region: assignment.value.clone(),
        method: match assignment.method {
            RegionMethod::Auto => RegionMethodDto::Auto,
            RegionMethod::Manual => RegionMethodDto::Manual,
        },
    }
}

fn map_error(e: source::SourceAppError) -> (StatusCode, Json<ErrorResponse>) {
    use source::SourceAppError as E;
    match e {
        E::NodeNotFound => err(NOT_FOUND, "node_not_found", "node does not exist"),
        E::InvalidInput(msg) => err(BAD_REQUEST, "invalid_input", msg),
        E::NameExists => err(CONFLICT, "name_exists", "name is already taken"),
        E::NodeChain(NodeChainError::Empty) => {
            err(BAD_REQUEST, "chain_empty", "chain must not be empty")
        }
        E::NodeChain(NodeChainError::SelfReference) => err(
            BAD_REQUEST,
            "chain_self_reference",
            "chain must not contain the node itself",
        ),
        E::NodeChain(NodeChainError::Duplicate(id)) => err(
            BAD_REQUEST,
            "chain_duplicate",
            &format!("chain contains duplicate node: {id}"),
        ),
        E::NodeChain(NodeChainError::NodeNotFound(ids)) => err(
            BAD_REQUEST,
            "chain_node_not_found",
            &format!("chain references non-existent nodes: {ids:?}"),
        ),
        E::NodeChain(NodeChainError::Cycle(path)) => err(
            CONFLICT,
            "chain_cycle",
            &format!("chain cycle detected: {path}"),
        ),
        E::Source(SourceError::TagNotFound) => {
            err(NOT_FOUND, "tag_not_found", "tag does not exist")
        }
        E::Source(SourceError::TagExists) => {
            err(CONFLICT, "tag_exists", "tag name is already taken")
        }
        E::Source(SourceError::NodeNotFound(id)) => err(
            NOT_FOUND,
            "node_not_found",
            &format!("node does not exist: {id}"),
        ),
        other => {
            tracing::warn!(error = %other, "node override command failed");
            err(INTERNAL_SERVER_ERROR, "internal", "internal error")
        }
    }
}

/// Register all node override, tag, and region routes on the given router.
pub fn register(
    router: utoipa_axum::router::OpenApiRouter<AppState>,
) -> utoipa_axum::router::OpenApiRouter<AppState> {
    use utoipa_axum::routes;
    router
        .routes(routes!(update_override))
        .routes(routes!(delete_override))
        .routes(routes!(set_region))
        .routes(routes!(set_node_chain))
        .routes(routes!(batch_set_enabled))
        .routes(routes!(set_node_tags))
        .routes(routes!(batch_set_tags))
        .routes(routes!(list_tags))
        .routes(routes!(create_tag))
        .routes(routes!(delete_tag))
}
