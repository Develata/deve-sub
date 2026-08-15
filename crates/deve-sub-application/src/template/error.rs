//! Template application errors.

use thiserror::Error;

use deve_sub_domain::TemplateError;

/// Errors produced by template application commands and queries.
#[derive(Debug, Error)]
pub enum TemplateAppError {
    /// Input validation failed (empty name, invalid spec, etc.).
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// A template was not found.
    #[error("template not found")]
    TemplateNotFound,

    /// A template version was not found.
    #[error("template version not found")]
    VersionNotFound,

    /// A rollback target version exists but belongs to a different template.
    /// The path `template_id` and the version's `template_id` must match;
    /// rejecting the mismatch prevents silently activating another template's
    /// version (F8.2).
    #[error("version does not belong to the specified template")]
    VersionTemplateMismatch,

    /// A template name is already taken.
    #[error("template name already exists")]
    NameExists,

    /// The template spec YAML could not be deserialized.
    #[error("spec YAML parse error: {0}")]
    SpecYamlParse(String),

    /// The template spec YAML exceeds the size limit.
    #[error("spec exceeds size limit: {0} bytes (max {1})")]
    SpecTooLarge(usize, usize),

    /// The template spec YAML alias nesting exceeds the depth limit.
    #[error("spec alias depth {0} exceeds limit {1}")]
    AliasDepthExceeded(u32, u32),

    /// The template spec contains a forbidden script tag.
    #[error("spec contains forbidden script tag: {0}")]
    ForbiddenScript(String),

    /// A template domain or storage operation failed.
    #[error(transparent)]
    Template(#[from] TemplateError),

    /// A node pool storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// The requested target profile is not recognized.
    #[error("unknown profile: {0}")]
    UnknownProfile(String),

    /// A generation pipeline semantic failure (e.g. strict-mode incompatible
    /// nodes).
    #[error(transparent)]
    Generation(#[from] deve_sub_domain::GenerationError),

    /// An emitter failed to produce output.
    #[error("emission error: {0}")]
    Emit(String),

    /// The generated output is empty or unparseable.
    #[error("generated output is empty or invalid")]
    EmptyOutput,

    /// The generated output parsed but failed structural validation for the
    /// target profile (e.g. mihomo output missing the `proxies` array, JSON
    /// profile emitting a scalar, uri_list with no lines).
    #[error("generated output failed structural validation: {0}")]
    InvalidStructure(String),

    /// No compatible nodes are available for generation. All resolved nodes
    /// were either excluded by the compatibility matrix or unavailable in the
    /// pool. Returning this error before cache mutation preserves the last
    /// successful generation (constraint #19).
    #[error("no compatible nodes available for generation")]
    NoCompatibleNodes,
}
