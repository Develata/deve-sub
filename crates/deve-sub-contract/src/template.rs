//! Template DTOs for the `/api/v1/templates` endpoints.
//!
//! These DTOs are the wire format for V3 subscription template management.
//! They are owned by the contract crate per ADR-0004: DTOs and `ToSchema`
//! derives live here, not in the API crate.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Request body for `POST /api/v1/templates`.
///
/// The `spec_yaml` field is the full V3 template document (apiVersion,
/// kind, metadata, spec) as a YAML string. The server validates it against
/// the M5 schema constraints before persistence (GEN-002).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateTemplateRequest {
    /// Human-readable template name.
    pub name: String,
    /// Optional description.
    #[serde(default)]
    pub description: String,
    /// The full V3 template YAML document.
    pub spec_yaml: String,
}

/// Request body for `PUT /api/v1/templates/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateTemplateRequest {
    /// Human-readable template name.
    pub name: String,
    /// Optional description.
    pub description: String,
    /// The full V3 template YAML document.
    pub spec_yaml: String,
}

/// Template information returned by template management endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TemplateDto {
    /// ULID identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Optional description.
    pub description: String,
    /// The active version number. `0` before the first version is committed.
    pub active_version: u64,
    /// The active version's ULID, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_version_id: Option<String>,
    /// Creation time (ISO 8601 UTC).
    pub created_at: String,
    /// Last update time (ISO 8601 UTC).
    pub updated_at: String,
}

/// Template version information.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TemplateVersionDto {
    /// ULID identifier.
    pub id: String,
    /// The template this version belongs to.
    pub template_id: String,
    /// Monotonic version number.
    pub version: u64,
    /// The spec YAML document.
    pub spec_yaml: String,
    /// Whether this is the active version.
    pub is_active: bool,
    /// Creation time (ISO 8601 UTC).
    pub created_at: String,
}

/// Response body for `POST /api/v1/templates` and `PUT /api/v1/templates/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TemplateResponse {
    /// The template aggregate.
    pub template: TemplateDto,
    /// The version created by this operation.
    pub version: TemplateVersionDto,
}

/// Response body for `GET /api/v1/templates` (cursor-paginated).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListTemplatesResponse {
    /// Templates in the current page.
    pub templates: Vec<TemplateDto>,
    /// Cursor for the next page (`None` if no more results).
    pub next_cursor: Option<String>,
}

/// Response body for `GET /api/v1/templates/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GetTemplateResponse {
    /// The template aggregate.
    pub template: TemplateDto,
}

/// Response body for `GET /api/v1/templates/{id}/versions`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListVersionsResponse {
    /// Versions, newest first.
    pub versions: Vec<TemplateVersionDto>,
}

/// Response body for `POST /api/v1/templates/{id}/rollback`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RollbackTemplateResponse {
    /// The activated version.
    pub version: TemplateVersionDto,
}

/// Query parameters for `GET /api/v1/templates`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListTemplatesQuery {
    /// Pagination cursor — the ULID of the last template from the previous
    /// page.
    pub cursor: Option<String>,
    /// Maximum number of templates to return (default 50, max 100).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// A node reference that could not be resolved to an active pool entry.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MissingNodeRefDto {
    /// The node ULID that was referenced.
    pub node_id: String,
    /// Why the node is unavailable: `not_found`, `missing_from_source`, or
    /// `inactive`.
    pub reason: String,
}

/// Resolution of a single proxy group's membership against the live pool.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GroupResolutionDto {
    /// The group name from the template spec.
    pub group_name: String,
    /// Node IDs from explicit `GroupMember::Node` entries that were found and
    /// active. Order matches the spec's `members` order.
    pub explicit_node_ids: Vec<String>,
    /// Node IDs auto-populated by the group's quick-group filter.
    pub quick_group_node_ids: Vec<String>,
    /// Explicit node references that could not be resolved.
    pub missing: Vec<MissingNodeRefDto>,
}

/// Response body for `GET /api/v1/templates/{id}/resolve`.
///
/// Resolves the template's `nodeSelector` and `proxyGroups` against the live
/// node pool. Read-only: no generation, no caching, no state change.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResolveTemplateResponse {
    /// Node IDs selected by the template's `nodeSelector`.
    pub selected_node_ids: Vec<String>,
    /// Node IDs from the selector that were referenced but unavailable.
    pub selection_missing: Vec<MissingNodeRefDto>,
    /// Per-group resolution for each `ProxyGroup` in the spec.
    pub groups: Vec<GroupResolutionDto>,
    /// Directed edges in the chain proxy dependency graph. Each edge
    /// represents a `from → to` dependency (relay sequence or group
    /// reference). Empty when the template has no relay groups or
    /// group-to-group references.
    pub chain_edges: Vec<ChainEdgeDto>,
}

/// A directed edge in the chain proxy dependency graph.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChainEdgeDto {
    /// Source vertex: `node:<ULID>` or `group:<name>`.
    pub from: String,
    /// Destination vertex: `node:<ULID>` or `group:<name>`.
    pub to: String,
}

/// A node excluded from generation because it is incompatible with the
/// requested target profile.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExcludedNodeDto {
    /// The node ULID.
    pub node_id: String,
    /// The node's display name at the time of exclusion.
    pub display_name: String,
    /// Why the node is incompatible (human-readable).
    pub reason: String,
}

/// Response body for `GET /api/v1/templates/{id}/compatibility`.
///
/// Reports which resolved nodes are included in and excluded from generation
/// for a given target profile. Incompatible nodes are never silently dropped
/// (constraint #7): they appear in `excluded` with a reason.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CompatibilityReportDto {
    /// The target profile: `mihomo`, `sing-box`, `xray`, `v2ray`,
    /// `shadowrocket`, or `uri_list`.
    pub profile: String,
    /// Node IDs that are compatible and will be included in generation.
    pub included_node_ids: Vec<String>,
    /// Nodes that are incompatible and excluded from generation.
    pub excluded: Vec<ExcludedNodeDto>,
}

/// Response body for `POST /api/v1/templates/{id}/generate`.
///
/// Contains the emitted subscription content for the requested target profile,
/// the compatibility report (included/excluded nodes), and any warnings
/// (missing references, empty pool). In strict mode, the request fails with
/// 422 instead of returning this body when any node is excluded (GEN-014).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GenerationResultDto {
    /// The emitted subscription content (YAML for mihomo, JSON for
    /// sing-box/xray/v2ray, base64 URI list for shadowrocket, plain URI list
    /// for uri_list).
    pub content: String,
    /// The target profile that was generated.
    pub profile: String,
    /// Node IDs included in the generated output.
    pub included_node_ids: Vec<String>,
    /// Nodes excluded from generation due to incompatibility.
    pub excluded: Vec<ExcludedNodeDto>,
    /// Non-fatal warnings: missing references, nodes that became unavailable
    /// during generation, or an empty compatible pool.
    pub warnings: Vec<String>,
}

/// Query parameters for `POST /api/v1/templates/{id}/generate`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct GenerateQuery {
    /// Target profile: `mihomo`, `sing-box`, `xray`, `v2ray`,
    /// `shadowrocket`, or `uri_list`.
    pub profile: String,
    /// Generation mode: `strict` (fail on incompatible nodes) or `lenient`
    /// (exclude and continue). Defaults to `lenient`.
    #[serde(default)]
    pub mode: Option<String>,
}

/// Query parameters for `GET /api/v1/templates/{id}/generations/active`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ActiveGenerationQuery {
    /// Target profile: `mihomo`, `sing-box`, `xray`, `v2ray`,
    /// `shadowrocket`, or `uri_list`.
    pub profile: String,
}

/// Response body for `GET /api/v1/templates/{id}/generations/active`.
///
/// Returns the currently active (last successfully published) generation for
/// the given template + profile. On generation failure, the previous active
/// generation remains served (GEN-015, constraint #19). Returns 404 if no
/// active generation exists.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ActiveGenerationResponse {
    /// The emitted subscription content.
    pub content: String,
    /// The target profile that was generated.
    pub profile: String,
    /// The template version the active generation was produced from.
    pub template_version: u64,
    /// The pool revision at the time of generation.
    pub pool_revision: u64,
    /// The cache key (SHA-256 hex) of the active entry.
    pub cache_key: String,
}

/// Request body for `POST /api/v1/templates/{id}/rollback`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RollbackRequest {
    /// The version ULID to activate.
    pub version_id: String,
}

/// Query parameters for `GET /api/v1/templates/{id}/compatibility`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CompatibilityQuery {
    /// Target profile: `mihomo`, `sing-box`, `xray`, `v2ray`, `shadowrocket`,
    /// or `uri_list`.
    pub profile: String,
}
