//! Node DTOs for the `/api/v1/nodes` endpoints.
//!
//! These DTOs are the wire format for node pool queries and manual import.
//! They are owned by the contract crate per ADR-0004: DTOs and `ToSchema`
//! derives live here, not in the API crate.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A node in the unified pool, as returned by list/get endpoints.
///
/// Pool metadata (`missing_from_source`, `is_active`, `revision`) is
/// included alongside the canonical node fields. Sensitive fields
/// (raw URI with embedded credentials) are never serialized.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeDto {
    /// ULID identifier.
    pub id: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Wire-level protocol kind (e.g. `Vless`, `Trojan`).
    pub protocol: String,
    /// Endpoint host (IPv6 bracketed in URI form).
    pub host: String,
    /// Endpoint port.
    pub port: u16,
    /// Region label, if assigned.
    pub region: Option<String>,
    /// Source label (the source name, or `"manual"` for pasted nodes).
    pub source_label: String,
    /// Import timestamp (ISO 8601 UTC).
    pub imported_at: String,
    /// Whether the node is active (eligible for generation).
    pub is_active: bool,
    /// Whether the node was marked missing after its source removed it.
    pub missing_from_source: bool,
    /// Optimistic-concurrency revision counter.
    pub revision: u64,
    /// Row creation time (ISO 8601 UTC).
    pub created_at: String,
    /// How the region was assigned (auto-detected or manual override).
    pub region_method: RegionMethodDto,
    /// Tags assigned to this node.
    pub tags: Vec<TagDto>,
}

/// Response body for `GET /api/v1/nodes` (cursor-paginated node list).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListNodesResponse {
    /// Nodes in the current page.
    pub nodes: Vec<NodeDto>,
    /// Cursor for the next page (`None` if no more results).
    pub next_cursor: Option<String>,
}

/// Response body for `GET /api/v1/nodes/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeResponse {
    /// The node.
    pub node: NodeDto,
}

/// Request body for `POST /api/v1/nodes/import` (NODE-001/002).
///
/// The `content` is a raw subscription payload (URI list, Base64, YAML, or
/// JSON). The server parses it with the same protocol parsers used for
/// source refresh and imports the resulting nodes into the pool with
/// dedup. `source_type` controls format detection; `auto` lets the server
/// detect from the content.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportNodesRequest {
    /// Raw subscription content to parse and import.
    pub content: String,
    /// Input format. `auto` lets the server detect.
    #[serde(default = "default_import_source_type")]
    pub source_type: super::source::SourceTypeDto,
}

fn default_import_source_type() -> super::source::SourceTypeDto {
    super::source::SourceTypeDto::Auto
}

/// Response body for `POST /api/v1/nodes/import`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ImportNodesResponse {
    /// Nodes newly inserted into the pool.
    pub new_nodes: u64,
    /// Nodes already present (duplicate of an existing active node).
    pub duplicate_nodes: u64,
    /// Input lines that could not be parsed.
    pub failed: u64,
    /// Per-line outcomes for diagnostics. Length equals the input line count.
    pub outcomes: Vec<ImportOutcomeDto>,
}

/// Per-line outcome of a manual import.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
pub enum ImportOutcomeDto {
    /// A new node was inserted; `data` is the node ULID.
    Inserted(String),
    /// The node was a duplicate; `data` is the existing node ULID.
    Duplicate(String),
    /// The line could not be parsed; `data` is the raw input text.
    Failed(String),
}

/// How a node's region was assigned.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RegionMethodDto {
    /// GeoIP-derived.
    Auto,
    /// Admin-authored override.
    Manual,
}

/// A user-defined tag.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TagDto {
    /// ULID identifier.
    pub id: String,
    /// Tag name (unique).
    pub name: String,
    /// Optional color for UI display.
    pub color: Option<String>,
}

/// Manual override applied to a node, as seen in API responses.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeOverrideDto {
    /// Override display name; `None` keeps the parsed name.
    pub display_name: Option<String>,
    /// Override region; `None` keeps the auto-detected region.
    pub region: Option<String>,
    /// Override enabled flag; `None` keeps natural status.
    pub enabled: Option<bool>,
    /// Override SNI.
    pub sni: Option<String>,
    /// Override skip-cert-verify.
    pub skip_cert_verify: Option<bool>,
    /// Override TLS fingerprint.
    pub fingerprint: Option<String>,
    /// Sort order for generation.
    pub sort_order: i64,
}

/// Request body for `PATCH /api/v1/nodes/{id}/override` (NODE-010).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateOverrideRequest {
    /// Override display name; `None` clears the override.
    pub display_name: Option<String>,
    pub region: Option<String>,
    pub enabled: Option<bool>,
    pub sni: Option<String>,
    pub skip_cert_verify: Option<bool>,
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub sort_order: i64,
}

/// Response body for override endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeOverrideResponse {
    #[serde(rename = "override")]
    pub override_: NodeOverrideDto,
}

/// Request body for `POST /api/v1/nodes/batch-enabled` (NODE-004).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BatchEnabledRequest {
    /// Node ULIDs to update.
    pub node_ids: Vec<String>,
    /// `true` to enable, `false` to disable.
    pub enabled: bool,
}

/// Response body for batch operations.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BatchResultDto {
    /// Number of rows affected.
    pub updated: u64,
}

/// One node's tag assignment in a batch tags request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeTagAssignmentDto {
    pub node_id: String,
    pub tag_ids: Vec<String>,
}

/// Request body for `POST /api/v1/nodes/batch-tags` (NODE-005).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BatchTagsRequest {
    pub assignments: Vec<NodeTagAssignmentDto>,
}

/// Request body for `PATCH /api/v1/nodes/{id}/region` (NODE-006).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetRegionRequest {
    /// `Some("US")` sets a manual region; `None` clears it.
    pub region: Option<String>,
}

/// Response body for region endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegionResponse {
    pub region: Option<String>,
    pub method: RegionMethodDto,
}

/// Request body for `POST /api/v1/tags`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateTagRequest {
    pub name: String,
    pub color: Option<String>,
}

/// Response body for tag creation.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TagResponse {
    pub tag: TagDto,
}

/// Response body for `GET /api/v1/tags`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListTagsResponse {
    pub tags: Vec<TagDto>,
}

/// Request body for `PUT /api/v1/nodes/{id}/tags`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetNodeTagsRequest {
    /// Tag IDs to assign to this node (replaces existing assignments).
    pub tag_ids: Vec<String>,
}
