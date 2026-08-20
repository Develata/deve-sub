//! DTO types for the templates page, matching `deve-sub-contract::template`.

#![cfg(target_family = "wasm")]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub active_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_version_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTemplatesResponse {
    pub templates: Vec<TemplateDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateTemplateRequest {
    pub name: String,
    pub description: String,
    pub spec_yaml: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateTemplateRequest {
    pub name: String,
    pub description: String,
    pub spec_yaml: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateVersionDto {
    pub id: String,
    pub template_id: String,
    pub version: u64,
    pub spec_yaml: String,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateResponse {
    pub template: TemplateDto,
    pub version: TemplateVersionDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTemplateResponse {
    pub template: TemplateDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListVersionsResponse {
    pub versions: Vec<TemplateVersionDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RollbackRequest {
    pub version_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackTemplateResponse {
    pub version: TemplateVersionDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcludedNodeDto {
    pub node_id: String,
    pub display_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationResultDto {
    pub content: String,
    pub profile: String,
    pub included_node_ids: Vec<String>,
    pub excluded: Vec<ExcludedNodeDto>,
    pub warnings: Vec<String>,
}

pub const PROFILES: &[&str] = &[
    "mihomo",
    "sing-box",
    "xray",
    "v2ray",
    "shadowrocket",
    "uri_list",
];

/// Modal state machine for the templates page.
#[derive(Clone, PartialEq)]
pub enum Modal {
    None,
    Create,
    Edit(TemplateDto),
    Delete(TemplateDto),
    Versions(TemplateDto),
    Rollback {
        template: TemplateDto,
        version: TemplateVersionDto,
    },
    Generate(TemplateDto),
    Preview(TemplateDto),
}
