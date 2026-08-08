//! Template application module: commands and queries for V3 subscription
//! templates.
//!
//! This module orchestrates domain services and port interfaces. It does not
//! execute SQL directly. See `docs/plan/03-architecture.md` §"Lightweight
//! CQRS" and `docs/plan/milestones/M5-generator-and-v3-template.md` for the
//! milestone blueprint.

pub mod commands;
pub mod compatibility;
pub mod error;
pub mod generation;
pub mod selection;
pub mod validation;

pub use commands::{
    CreateTemplateParams, CreateTemplateResult, UpdateTemplateParams, UpdateTemplateResult,
    create_template, delete_template, get_active_version, get_template, get_template_by_name,
    list_templates, list_versions, rollback_template, update_template,
};
pub use compatibility::check_compatibility;
pub use error::TemplateAppError;
pub use generation::{generate, get_active_generation};
pub use selection::{apply_sort_order, resolve_group, resolve_selection, resolve_template};
pub use validation::{parse_template_document, validate_document};
