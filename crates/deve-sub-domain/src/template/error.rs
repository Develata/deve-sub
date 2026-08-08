//! Template domain errors.

use thiserror::Error;

/// Errors produced by template operations.
#[derive(Debug, Error)]
pub enum TemplateError {
    /// A template was not found.
    #[error("template not found")]
    TemplateNotFound,

    /// A template name is already taken.
    #[error("template name already exists")]
    NameExists,

    /// A template version was not found.
    #[error("template version not found")]
    VersionNotFound,

    /// The template spec is invalid (schema, limits, or structural).
    #[error("invalid template spec: {0}")]
    InvalidSpec(String),

    /// A template spec YAML exceeds the size limit.
    #[error("template spec exceeds size limit: {0} bytes")]
    SpecTooLarge(u64),

    /// A template spec YAML alias nesting exceeds the depth limit.
    #[error("template spec alias depth {0} exceeds limit {1}")]
    AliasDepthExceeded(u32, u32),

    /// The template spec contains a forbidden script tag.
    #[error("template spec contains forbidden script tag: {0}")]
    ForbiddenScript(String),

    /// A proxy group name is duplicated.
    #[error("duplicate proxy group name: {0}")]
    DuplicateGroupName(String),

    /// A proxy group references an unknown group.
    #[error("proxy group references unknown group: {0}")]
    UnknownGroupReference(String),

    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),
}
