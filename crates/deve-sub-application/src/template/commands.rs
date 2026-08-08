//! Template application commands and queries: create, update, delete, list,
//! get, rollback, version history.
//!
//! These functions orchestrate domain services and port interfaces. They do
//! not execute SQL directly. One API operation maps to one command. See
//! `docs/plan/03-architecture.md` §"Lightweight CQRS".

use deve_sub_domain::{
    SubscriptionTemplate, TemplateDocument, TemplateRepository, TemplateVersion,
    TemplateVersionRepository,
};
use deve_sub_kernel::{TemplateId, TemplateVersionId, Timestamp};

use super::error::TemplateAppError;
use super::validation::{map_template_error, validate_document};

/// Maximum template name length.
const MAX_NAME_LEN: usize = 128;

/// Maximum description length.
const MAX_DESC_LEN: usize = 2048;

/// Default page size for list queries.
const DEFAULT_LIST_LIMIT: u32 = 50;

/// Validate a template name at the application boundary.
fn validate_name(name: &str) -> Result<(), TemplateAppError> {
    if name.is_empty() {
        return Err(TemplateAppError::InvalidInput(
            "name must not be empty".to_owned(),
        ));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(TemplateAppError::InvalidInput(format!(
            "name must not exceed {MAX_NAME_LEN} characters"
        )));
    }
    Ok(())
}

/// Validate a template description at the application boundary.
fn validate_description(desc: &str) -> Result<(), TemplateAppError> {
    if desc.len() > MAX_DESC_LEN {
        return Err(TemplateAppError::InvalidInput(format!(
            "description must not exceed {MAX_DESC_LEN} characters"
        )));
    }
    Ok(())
}

/// Parameters for [`create_template`].
pub struct CreateTemplateParams {
    /// Human-readable name.
    pub name: String,
    /// Optional description.
    pub description: String,
    /// The full V3 template YAML document.
    pub spec_yaml: String,
}

/// Result of a successful template creation.
#[derive(Debug, Clone)]
pub struct CreateTemplateResult {
    /// The created template aggregate.
    pub template: SubscriptionTemplate,
    /// The first version (version 1) containing the initial spec.
    pub version: TemplateVersion,
}

/// Create a new V3 subscription template.
///
/// Validates the name, parses and validates the spec YAML against the M5
/// schema constraints, persists the template aggregate, and commits the first
/// version (version 1) as the active version. No partial template is stored
/// on validation failure (GEN-002).
///
/// # Errors
/// - [`TemplateAppError::InvalidInput`] — name or spec validation failed.
/// - [`TemplateAppError::SpecYamlParse`] — the YAML could not be parsed.
/// - [`TemplateAppError::NameExists`] — name collision.
/// - [`TemplateAppError::Template`] — storage error.
pub async fn create_template(
    template_repo: &dyn TemplateRepository,
    version_repo: &dyn TemplateVersionRepository,
    params: CreateTemplateParams,
) -> Result<CreateTemplateResult, TemplateAppError> {
    validate_name(&params.name)?;
    validate_description(&params.description)?;

    let doc: TemplateDocument = serde_yaml::from_str(&params.spec_yaml)
        .map_err(|e| TemplateAppError::SpecYamlParse(e.to_string()))?;
    validate_document(&doc, &params.spec_yaml)?;

    let mut template = SubscriptionTemplate::new(&params.name, &params.description);

    let version = TemplateVersion {
        id: TemplateVersionId::new(),
        template_id: template.id,
        version: 1,
        spec: doc.spec.clone(),
        spec_yaml: params.spec_yaml,
        is_active: true,
        created_at: Timestamp::now(),
    };

    template.active_version_id = Some(version.id);
    template.active_version = 1;

    template_repo
        .create(&template)
        .await
        .map_err(map_template_error)?;
    version_repo
        .create(&version)
        .await
        .map_err(map_template_error)?;

    Ok(CreateTemplateResult { template, version })
}

/// Parameters for [`update_template`].
pub struct UpdateTemplateParams {
    /// ID of the template to update.
    pub id: TemplateId,
    /// New name.
    pub name: String,
    /// New description.
    pub description: String,
    /// New full V3 template YAML document.
    pub spec_yaml: String,
}

/// Result of a successful template update.
#[derive(Debug, Clone)]
pub struct UpdateTemplateResult {
    /// The updated template aggregate.
    pub template: SubscriptionTemplate,
    /// The new version created by this update.
    pub version: TemplateVersion,
}

/// Update an existing template, creating a new version.
///
/// Loads the template, validates the new name and spec YAML, determines the
/// next version number, commits the new version as active, and updates the
/// aggregate's metadata. The previous version is deactivated atomically by
/// the version repository's `create` transaction (GEN-003).
///
/// # Errors
/// - [`TemplateAppError::TemplateNotFound`] — template does not exist.
/// - [`TemplateAppError::InvalidInput`] — validation failed.
/// - [`TemplateAppError::NameExists`] — name collision.
/// - [`TemplateAppError::Template`] — storage error.
pub async fn update_template(
    template_repo: &dyn TemplateRepository,
    version_repo: &dyn TemplateVersionRepository,
    params: UpdateTemplateParams,
) -> Result<UpdateTemplateResult, TemplateAppError> {
    validate_name(&params.name)?;
    validate_description(&params.description)?;

    let doc: TemplateDocument = serde_yaml::from_str(&params.spec_yaml)
        .map_err(|e| TemplateAppError::SpecYamlParse(e.to_string()))?;
    validate_document(&doc, &params.spec_yaml)?;

    let mut template = template_repo
        .find_by_id(params.id)
        .await
        .map_err(map_template_error)?
        .ok_or(TemplateAppError::TemplateNotFound)?;

    template.name = params.name;
    template.description = params.description;
    template.updated_at = Timestamp::now();

    let next_version = template.active_version + 1;
    let version = TemplateVersion {
        id: TemplateVersionId::new(),
        template_id: template.id,
        version: next_version,
        spec: doc.spec.clone(),
        spec_yaml: params.spec_yaml,
        is_active: true,
        created_at: Timestamp::now(),
    };

    template.active_version_id = Some(version.id);
    template.active_version = next_version;

    version_repo
        .create(&version)
        .await
        .map_err(map_template_error)?;
    template_repo
        .update(&template)
        .await
        .map_err(map_template_error)?;

    Ok(UpdateTemplateResult { template, version })
}

/// Delete a template by ID.
///
/// Returns [`TemplateAppError::TemplateNotFound`] if the template does not
/// exist. The storage layer cascades the deletion to all versions and
/// generation cache entries.
///
/// # Errors
/// - [`TemplateAppError::TemplateNotFound`] — template does not exist.
/// - [`TemplateAppError::Template`] — storage error.
pub async fn delete_template(
    repo: &dyn TemplateRepository,
    id: TemplateId,
) -> Result<(), TemplateAppError> {
    repo.delete(id).await.map_err(map_template_error)?;
    Ok(())
}

/// Get a template by ID.
///
/// # Errors
/// - [`TemplateAppError::Template`] — storage error.
pub async fn get_template(
    repo: &dyn TemplateRepository,
    id: TemplateId,
) -> Result<Option<SubscriptionTemplate>, TemplateAppError> {
    repo.find_by_id(id).await.map_err(map_template_error)
}

/// Get a template by name.
///
/// # Errors
/// - [`TemplateAppError::Template`] — storage error.
pub async fn get_template_by_name(
    repo: &dyn TemplateRepository,
    name: &str,
) -> Result<Option<SubscriptionTemplate>, TemplateAppError> {
    repo.find_by_name(name).await.map_err(map_template_error)
}

/// List templates with cursor pagination.
///
/// Returns up to `limit` templates whose ULID is greater than `cursor` (or
/// all if `cursor` is `None`). The caller derives the next cursor from the
/// last element's ID.
///
/// # Errors
/// - [`TemplateAppError::Template`] — storage error.
pub async fn list_templates(
    repo: &dyn TemplateRepository,
    cursor: Option<TemplateId>,
    limit: Option<u32>,
) -> Result<Vec<SubscriptionTemplate>, TemplateAppError> {
    let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT);
    repo.list(cursor, limit).await.map_err(map_template_error)
}

/// Get the active version of a template.
///
/// # Errors
/// - [`TemplateAppError::Template`] — storage error.
pub async fn get_active_version(
    repo: &dyn TemplateVersionRepository,
    template_id: TemplateId,
) -> Result<Option<TemplateVersion>, TemplateAppError> {
    repo.find_active(template_id)
        .await
        .map_err(map_template_error)
}

/// List version history for a template, newest first.
///
/// # Errors
/// - [`TemplateAppError::Template`] — storage error.
pub async fn list_versions(
    repo: &dyn TemplateVersionRepository,
    template_id: TemplateId,
    limit: Option<u32>,
) -> Result<Vec<TemplateVersion>, TemplateAppError> {
    let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT);
    repo.list_for_template(template_id, limit)
        .await
        .map_err(map_template_error)
}

/// Rollback a template to a specific version.
///
/// Activates the specified version, deactivating the currently active one.
/// The aggregate's `active_version_id` and `active_version` are updated to
/// reflect the rollback. The version history is preserved — no versions are
/// deleted (GEN-004).
///
/// # Errors
/// - [`TemplateAppError::VersionNotFound`] — the version ID does not exist.
/// - [`TemplateAppError::Template`] — storage error.
pub async fn rollback_template(
    version_repo: &dyn TemplateVersionRepository,
    version_id: TemplateVersionId,
) -> Result<TemplateVersion, TemplateAppError> {
    version_repo
        .activate(version_id)
        .await
        .map_err(map_template_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deve_sub_domain::MAX_SPEC_BYTES;

    const VALID_YAML: &str = "\
apiVersion: deve-sub.io/v1
kind: SubscriptionTemplate

metadata:
  name: default-mihomo
  description: Default Mihomo template
  version: 1

spec:
  targetProfiles:
    - mihomo
  variables: {}
  nodeSelector:
    mode: dynamic
  proxyGroups: []
  rules: []
  dns: {}
  tun: {}
  output: {}
";

    #[test]
    fn validate_accepts_valid_document() {
        let doc: TemplateDocument = serde_yaml::from_str(VALID_YAML).expect("parse");
        validate_document(&doc, VALID_YAML).expect("should pass");
    }

    #[test]
    fn validate_rejects_wrong_api_version() {
        let yaml = VALID_YAML.replace("deve-sub.io/v1", "v2");
        let doc: TemplateDocument = serde_yaml::from_str(&yaml).expect("parse");
        let err = validate_document(&doc, &yaml).expect_err("should fail");
        assert!(matches!(err, TemplateAppError::InvalidInput(_)));
    }

    #[test]
    fn validate_rejects_empty_name() {
        let yaml = VALID_YAML.replace("default-mihomo", "");
        let doc: TemplateDocument = serde_yaml::from_str(&yaml).expect("parse");
        let err = validate_document(&doc, &yaml).expect_err("should fail");
        assert!(matches!(err, TemplateAppError::InvalidInput(_)));
    }

    #[test]
    fn validate_rejects_script_tag() {
        let yaml = VALID_YAML.replace(
            "  proxyGroups: []",
            "  proxyGroups: []\n  script: \"require('child_process').exec('rm -rf /')\"",
        );
        let doc: TemplateDocument = serde_yaml::from_str(&yaml).expect("parse");
        let err = validate_document(&doc, &yaml).expect_err("should fail");
        assert!(matches!(err, TemplateAppError::ForbiddenScript(_)));
    }

    #[test]
    fn validate_rejects_duplicate_group_names() {
        let yaml = VALID_YAML.replace(
            "  proxyGroups: []",
            "  proxyGroups:\n    - name: proxy\n      type: select\n      members: []\n    - name: proxy\n      type: url-test\n      members: []",
        );
        let doc: TemplateDocument = serde_yaml::from_str(&yaml).expect("parse");
        let err = validate_document(&doc, &yaml).expect_err("should fail");
        assert!(matches!(err, TemplateAppError::InvalidInput(_)));
    }

    #[test]
    fn validate_rejects_unknown_group_reference() {
        let yaml = VALID_YAML.replace(
            "  proxyGroups: []",
            "  proxyGroups:\n    - name: proxy\n      type: relay\n      members:\n        - kind: group\n          name: nonexistent",
        );
        let doc: TemplateDocument = serde_yaml::from_str(&yaml).expect("parse");
        let err = validate_document(&doc, &yaml).expect_err("should fail");
        assert!(matches!(err, TemplateAppError::InvalidInput(_)));
    }

    #[test]
    fn validate_rejects_oversized_spec() {
        // Build a YAML doc that exceeds 1 MiB by padding the description.
        let pad = "x".repeat(MAX_SPEC_BYTES + 100);
        let yaml = format!("{VALID_YAML}\n# padding\n# {pad}\n");
        let doc: TemplateDocument = serde_yaml::from_str(VALID_YAML).expect("parse");
        let err = validate_document(&doc, &yaml).expect_err("should fail");
        assert!(matches!(err, TemplateAppError::SpecTooLarge(_, _)));
    }
}
