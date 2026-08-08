//! Template aggregate and version entity.
//!
//! A `SubscriptionTemplate` is the aggregate root: it carries the mutable
//! metadata (name, description) and references its version history. Each edit
//! creates a new `TemplateVersion` capturing the `TemplateSpec` snapshot. The
//! active version is the one served by generation; rollback re-points the
//! active version to a prior snapshot without deleting history.

use deve_sub_kernel::{TemplateId, TemplateVersionId, Timestamp};

use super::spec::TemplateSpec;

/// The template aggregate root.
///
/// Represents a V3 subscription template. The current spec lives in the
/// active version; the aggregate itself holds only identity and metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionTemplate {
    /// Unique identifier (ULID).
    pub id: TemplateId,
    /// Human-readable name, unique across the deployment.
    pub name: String,
    /// Optional description.
    pub description: String,
    /// The currently active version ID. `None` only transiently before the
    /// first version is committed.
    pub active_version_id: Option<TemplateVersionId>,
    /// The active version number (monotonic). `0` before the first version.
    pub active_version: u64,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last update time.
    pub updated_at: Timestamp,
}

impl SubscriptionTemplate {
    /// Create a new template shell. The first version is committed separately
    /// by the application layer via [`super::ports::TemplateVersionRepository`].
    #[must_use]
    pub fn new(name: &str, description: &str) -> Self {
        let now = Timestamp::now();
        Self {
            id: TemplateId::new(),
            name: name.to_owned(),
            description: description.to_owned(),
            active_version_id: None,
            active_version: 0,
            created_at: now,
            updated_at: now,
        }
    }
}

/// A versioned snapshot of a template's spec.
///
/// Each create or update produces a new `TemplateVersion` with a monotonic
/// version number. The spec is stored as both a parsed `TemplateSpec` (for
/// in-process validation and generation) and as the original YAML text (for
/// round-trip fidelity and export).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateVersion {
    /// Unique identifier (ULID).
    pub id: TemplateVersionId,
    /// The template this version belongs to.
    pub template_id: TemplateId,
    /// Monotonic version number per template, starting at 1.
    pub version: u64,
    /// The parsed spec.
    pub spec: TemplateSpec,
    /// The original YAML text of the full V3 document, for round-trip and
    /// export.
    pub spec_yaml: String,
    /// Whether this is the active version for its template.
    pub is_active: bool,
    /// Creation time.
    pub created_at: Timestamp,
}
