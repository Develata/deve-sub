//! V3 template spec validation at the application boundary.
//!
//! Validates a parsed [`TemplateDocument`] against the M5 blueprint
//! constraints before persistence. The checks are:
//!
//! - Size: serialized YAML ≤ 1 MiB (SEC-005 parity).
//! - Structural nesting depth: ≤ 10 (SEC-005 parity). serde_yaml 0.9
//!   resolves YAML aliases during parsing, so this checks raw nesting
//!   depth as a defense against pathologically deep documents.
//! - No script tags: forbidden keys `script`, `script_str`, `shell`,
//!   `exec`, `eval`, `javascript`, `lua` anywhere in the document
//!   (constraint #10).
//! - API version and kind match the V3 namespace.
//! - Proxy group names unique within the template.
//! - Group member references resolve to a known group name.
//!
//! Schema validation runs before persistence; no partial template is stored
//! (GEN-002).

use serde_yaml::Value;

use deve_sub_domain::{
    API_VERSION, GroupMember, KIND, MAX_ALIAS_DEPTH, MAX_SPEC_BYTES, TemplateDocument,
    TemplateError,
};

use super::error::TemplateAppError;

/// Forbidden top-level keys anywhere in the YAML tree that would allow
/// arbitrary script execution. Matched case-sensitively against map keys.
const FORBIDDEN_SCRIPT_KEYS: &[&str] = &[
    "script",
    "script_str",
    "shell",
    "exec",
    "eval",
    "javascript",
    "lua",
];

/// Validate a parsed V3 template document against the M5 schema constraints.
///
/// Returns the first violation as [`TemplateAppError`], or `Ok(())` if the
/// document passes all checks.
///
/// # Errors
/// - [`TemplateAppError::InvalidInput`] — missing apiVersion/kind, empty
///   name, duplicate group name, or unknown group reference.
/// - [`TemplateAppError::SpecTooLarge`] — serialized YAML exceeds 1 MiB.
/// - [`TemplateAppError::AliasDepthExceeded`] — structural nesting > 10.
/// - [`TemplateAppError::ForbiddenScript`] — a forbidden script key is
///   present.
pub fn validate_document(doc: &TemplateDocument, spec_yaml: &str) -> Result<(), TemplateAppError> {
    // Size limit.
    let size = spec_yaml.len();
    if size > MAX_SPEC_BYTES {
        return Err(TemplateAppError::SpecTooLarge(size, MAX_SPEC_BYTES));
    }

    // API version and kind.
    if doc.api_version != API_VERSION {
        return Err(TemplateAppError::InvalidInput(format!(
            "apiVersion must be '{API_VERSION}', got '{}'",
            doc.api_version
        )));
    }
    if doc.kind != KIND {
        return Err(TemplateAppError::InvalidInput(format!(
            "kind must be '{KIND}', got '{}'",
            doc.kind
        )));
    }

    // Name non-empty.
    if doc.metadata.name.is_empty() {
        return Err(TemplateAppError::InvalidInput(
            "metadata.name must not be empty".to_owned(),
        ));
    }

    // Parse the raw YAML for depth and forbidden-key checks. These operate
    // on the raw tree because the typed TemplateSpec erases structural
    // detail during deserialization.
    let raw: Value = serde_yaml::from_str(spec_yaml)
        .map_err(|e| TemplateAppError::SpecYamlParse(e.to_string()))?;

    check_depth(&raw, 0)?;
    check_forbidden_scripts(&raw)?;

    // Proxy group name uniqueness and member reference validity.
    let group_names: std::collections::HashSet<&str> = doc
        .spec
        .proxy_groups
        .iter()
        .map(|g| g.name.as_str())
        .collect();
    if group_names.len() != doc.spec.proxy_groups.len() {
        let mut seen = std::collections::HashSet::new();
        for g in &doc.spec.proxy_groups {
            if !seen.insert(g.name.as_str()) {
                return Err(TemplateAppError::InvalidInput(format!(
                    "duplicate proxy group name: {}",
                    g.name
                )));
            }
        }
    }

    for group in &doc.spec.proxy_groups {
        for member in &group.members {
            if let GroupMember::Group { name } = member
                && !group_names.contains(name.as_str())
            {
                return Err(TemplateAppError::InvalidInput(format!(
                    "proxy group '{}' references unknown group '{}'",
                    group.name, name
                )));
            }
        }
    }

    Ok(())
}

/// Recursively check structural nesting depth. WHY: serde_yaml 0.9 resolves
/// YAML aliases during parsing, so the `Value` tree contains no `Alias`
/// variant. We check raw nesting depth (mappings/sequences/each tag level)
/// as a defense against pathologically deep documents that could exhaust
/// stack space during downstream processing. The limit mirrors the SEC-005
/// alias-depth parity requirement.
fn check_depth(value: &Value, depth: u32) -> Result<(), TemplateAppError> {
    let next_depth = depth + 1;
    if next_depth > MAX_ALIAS_DEPTH {
        return Err(TemplateAppError::AliasDepthExceeded(
            next_depth,
            MAX_ALIAS_DEPTH,
        ));
    }
    match value {
        Value::Mapping(map) => {
            for (k, v) in map {
                check_depth(k, depth)?;
                check_depth(v, next_depth)?;
            }
        }
        Value::Sequence(seq) => {
            for v in seq {
                check_depth(v, next_depth)?;
            }
        }
        Value::Tagged(tagged) => {
            check_depth(&tagged.value, next_depth)?;
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

/// Walk the YAML tree and reject any map key matching a forbidden script key.
fn check_forbidden_scripts(value: &Value) -> Result<(), TemplateAppError> {
    match value {
        Value::Mapping(map) => {
            for (k, v) in map {
                if let Value::String(key) = k
                    && FORBIDDEN_SCRIPT_KEYS.contains(&key.as_str())
                {
                    return Err(TemplateAppError::ForbiddenScript(key.clone()));
                }
                check_forbidden_scripts(v)?;
            }
        }
        Value::Sequence(seq) => {
            for v in seq {
                check_forbidden_scripts(v)?;
            }
        }
        Value::Tagged(tagged) => {
            check_forbidden_scripts(&tagged.value)?;
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

/// Map domain errors to application errors for non-create operations.
pub(super) fn map_template_error(e: TemplateError) -> TemplateAppError {
    match e {
        TemplateError::TemplateNotFound => TemplateAppError::TemplateNotFound,
        TemplateError::VersionNotFound => TemplateAppError::VersionNotFound,
        TemplateError::NameExists => TemplateAppError::NameExists,
        other => TemplateAppError::Template(other),
    }
}
