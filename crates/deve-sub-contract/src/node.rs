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
