//! Node management DTOs and modal state for `/api/v1/nodes/*`.
//!
//! These mirror `deve_sub_contract::node` and `deve_sub_contract::source`
//! for the web crate. The frontend is a thin shell — no business logic here,
//! only wire types and UI modal state.

#![cfg(target_family = "wasm")]

use serde::{Deserialize, Serialize};

/// How a node's region was assigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionMethodDto {
    Auto,
    Manual,
}

/// Input format of a subscription source (mirrors `SourceTypeDto`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTypeDto {
    Auto,
    Base64,
    UriList,
    MihomoYaml,
    SingboxJson,
    XrayJson,
    V2rayJson,
    Shadowrocket,
}

impl SourceTypeDto {
    /// All variants in a stable display order (matches contract).
    #[must_use]
    pub const fn all() -> &'static [SourceTypeDto] {
        &[
            Self::Auto,
            Self::Base64,
            Self::UriList,
            Self::MihomoYaml,
            Self::SingboxJson,
            Self::XrayJson,
            Self::V2rayJson,
            Self::Shadowrocket,
        ]
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Base64 => "base64",
            Self::UriList => "uri_list",
            Self::MihomoYaml => "mihomo_yaml",
            Self::SingboxJson => "singbox_json",
            Self::XrayJson => "xray_json",
            Self::V2rayJson => "v2ray_json",
            Self::Shadowrocket => "shadowrocket",
        }
    }
}

/// A user-defined tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagDto {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// A node in the unified pool (full contract shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDto {
    pub id: String,
    pub display_name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    pub source_label: String,
    pub is_active: bool,
    pub missing_from_source: bool,
    pub region_method: RegionMethodDto,
    pub tags: Vec<TagDto>,
    pub chain: Vec<String>,
}

/// Response body for `GET /api/v1/nodes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListNodesResponse {
    pub nodes: Vec<NodeDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Request body for `POST /api/v1/nodes/import`.
#[derive(Debug, Clone, Serialize)]
pub struct ImportNodesRequest {
    pub content: String,
    pub source_type: SourceTypeDto,
}

/// Response body for `POST /api/v1/nodes/import`.
#[derive(Debug, Clone, Deserialize)]
pub struct ImportNodesResponse {
    pub new_nodes: u64,
    pub duplicate_nodes: u64,
    pub failed: u64,
}

/// Request body for `PATCH /api/v1/nodes/{id}/override`.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateOverrideRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sni: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_cert_verify: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub sort_order: i64,
}

/// Request body for `POST /api/v1/nodes/batch-enabled`.
#[derive(Debug, Clone, Serialize)]
pub struct BatchEnabledRequest {
    pub node_ids: Vec<String>,
    pub enabled: bool,
}

/// Response body for batch operations.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResultDto {
    pub updated: u64,
}

/// One node's tag assignment in a batch tags request.
#[derive(Debug, Clone, Serialize)]
pub struct NodeTagAssignmentDto {
    pub node_id: String,
    pub tag_ids: Vec<String>,
}

/// Request body for `POST /api/v1/nodes/batch-tags`.
#[derive(Debug, Clone, Serialize)]
pub struct BatchTagsRequest {
    pub assignments: Vec<NodeTagAssignmentDto>,
}

/// Request body for `PUT /api/v1/nodes/{id}/tags`.
#[derive(Debug, Clone, Serialize)]
pub struct SetNodeTagsRequest {
    pub tag_ids: Vec<String>,
}

/// Request body for `PATCH /api/v1/nodes/{id}/region`.
#[derive(Debug, Clone, Serialize)]
pub struct SetRegionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

/// Response body for region endpoints.
#[derive(Debug, Clone, Deserialize)]
pub struct RegionResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    pub method: RegionMethodDto,
}

/// Request body for `PUT /api/v1/nodes/{id}/chain`.
#[derive(Debug, Clone, Serialize)]
pub struct SetNodeChainRequest {
    pub nodes: Vec<String>,
}

/// Response body for chain endpoints.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeChainResponse {
    pub nodes: Vec<String>,
}

/// Response body for `GET /api/v1/tags`.
#[derive(Debug, Clone, Deserialize)]
pub struct ListTagsResponse {
    pub tags: Vec<TagDto>,
}

/// Request body for `POST /api/v1/tags`.
#[derive(Debug, Clone, Serialize)]
pub struct CreateTagRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Response body for tag creation.
#[derive(Debug, Clone, Deserialize)]
pub struct TagResponse {
    pub tag: TagDto,
}

/// Which modal is open on the Nodes page.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeModal {
    /// Modal is closed.
    None,
    /// Import nodes (manual paste).
    Import,
    /// Assign tags to nodes (single or batch). Carries node ULIDs.
    Tags(Vec<String>),
    /// Set manual region on a single node.
    SetRegion(String),
    /// Edit manual override on a single node.
    Override(String),
    /// Edit proxy chain on a single node. Carries node ID and the
    /// current chain (ordered node IDs) for initial display.
    Chain(String, Vec<String>),
}
