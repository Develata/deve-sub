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
