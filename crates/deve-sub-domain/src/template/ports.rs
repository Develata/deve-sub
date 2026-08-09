//! Port traits for template and template version storage.

use async_trait::async_trait;

use deve_sub_kernel::{TemplateId, TemplateVersionId};

use super::error::TemplateError;
use super::{SubscriptionTemplate, TemplateVersion};

/// Storage boundary for template aggregates.
#[async_trait]
pub trait TemplateRepository: Send + Sync {
    /// Create a new template. Returns [`TemplateError::NameExists`] if the
    /// name is already taken.
    async fn create(&self, template: &SubscriptionTemplate) -> Result<(), TemplateError>;

    /// Find a template by ID.
    async fn find_by_id(
        &self,
        id: TemplateId,
    ) -> Result<Option<SubscriptionTemplate>, TemplateError>;

    /// Find a template by name.
    async fn find_by_name(&self, name: &str)
    -> Result<Option<SubscriptionTemplate>, TemplateError>;

    /// List templates with cursor pagination by `TemplateId`.
    async fn list(
        &self,
        cursor: Option<TemplateId>,
        limit: u32,
    ) -> Result<Vec<SubscriptionTemplate>, TemplateError>;

    /// Update a template's metadata (name, description, active version,
    /// `updated_at`). Returns [`TemplateError::TemplateNotFound`] if the
    /// template does not exist.
    async fn update(&self, template: &SubscriptionTemplate) -> Result<(), TemplateError>;

    /// Delete a template and all its versions.
    async fn delete(&self, id: TemplateId) -> Result<(), TemplateError>;
}

/// Storage boundary for template version snapshots.
#[async_trait]
pub trait TemplateVersionRepository: Send + Sync {
    /// Create a new version and deactivate the previous active version for
    /// the same template in a single transaction. The caller is responsible
    /// for assigning the monotonic `version` number.
    async fn create(&self, version: &TemplateVersion) -> Result<(), TemplateError>;

    /// Find the active version for a template.
    async fn find_active(
        &self,
        template_id: TemplateId,
    ) -> Result<Option<TemplateVersion>, TemplateError>;

    /// Find a specific version by ID.
    async fn find_by_id(
        &self,
        id: TemplateVersionId,
    ) -> Result<Option<TemplateVersion>, TemplateError>;

    /// Find a specific version by `(template_id, version_number)`. Used by
    /// Subscription delivery when `template_version_pin` is set. Returns
    /// `None` if the template has no version with that number.
    async fn find_by_version_number(
        &self,
        template_id: TemplateId,
        version: u64,
    ) -> Result<Option<TemplateVersion>, TemplateError>;

    /// List versions for a template, newest first.
    async fn list_for_template(
        &self,
        template_id: TemplateId,
        limit: u32,
    ) -> Result<Vec<TemplateVersion>, TemplateError>;

    /// Activate a specific version, deactivating the currently active one.
    /// Used by rollback. Returns [`TemplateError::VersionNotFound`] if the
    /// version ID does not exist.
    async fn activate(
        &self,
        version_id: TemplateVersionId,
    ) -> Result<TemplateVersion, TemplateError>;
}
