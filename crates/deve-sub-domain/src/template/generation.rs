//! Generation pipeline domain types.
//!
//! These types model the inputs and outputs of the generation pipeline
//! (resolve → compat → strict check → emit → validate). The pipeline
//! orchestration lives in the application layer; this module defines the
//! domain vocabulary it operates on.
//!
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"Generation
//! pipeline".

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use deve_sub_kernel::{NodeId, TemplateId};

use super::spec::NodeSelector;

/// Whether generation fails or proceeds when incompatible nodes are present.
///
/// `Strict` returns [`GenerationError::IncompatibleNodes`] if any node is
/// excluded. `Lenient` excludes incompatible nodes and continues, reporting
/// them in [`GenerationResult::excluded`] (constraint #7: no silent dropping).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GenerationMode {
    /// Fail generation if any node is incompatible (GEN-014).
    Strict,
    /// Exclude incompatible nodes and continue (default).
    #[default]
    Lenient,
}

impl GenerationMode {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Lenient => "lenient",
        }
    }
}

impl std::fmt::Display for GenerationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for GenerationMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "strict" => Ok(Self::Strict),
            "lenient" => Ok(Self::Lenient),
            other => Err(format!("unknown generation mode: {other}")),
        }
    }
}

/// A generation request: which template, which target profile, which mode.
///
/// `profile` is a kebab-case string (e.g. `"mihomo"`) parsed to
/// [`deve_sub_compatibility::ProfileKind`] by the application layer. It is a
/// `String` here because the domain cannot depend on the compatibility crate.
///
/// `node_selection` overrides the template's `nodeSelector` when set. This is
/// used by Subscription delivery: a Subscription is an independent aggregate
/// that owns its selection, and delivery generates against the Subscription's
/// selection, not the template's. `None` uses the template's `nodeSelector`
/// (the M5 admin-generate path).
///
/// `template_version_pin` selects a specific template version instead of the
/// active one. `None` follows the template's active version (M5 behavior).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationRequest {
    pub template_id: TemplateId,
    pub profile: String,
    pub mode: GenerationMode,
    /// Override the template's `nodeSelector`. `None` = use the template's.
    pub node_selection: Option<NodeSelector>,
    /// Pin a specific template version. `None` = use the active version.
    pub template_version_pin: Option<u64>,
}

impl GenerationRequest {
    /// Construct a generation request using the template's own selection and
    /// the active version (the M5 admin-generate path).
    #[must_use]
    pub fn new(template_id: TemplateId, profile: String, mode: GenerationMode) -> Self {
        Self {
            template_id,
            profile,
            mode,
            node_selection: None,
            template_version_pin: None,
        }
    }
}

/// The result of a successful generation: emitted content plus the
/// compatibility report and any warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationResult {
    pub content: String,
    pub profile: String,
    pub included_node_ids: Vec<NodeId>,
    pub excluded: Vec<ExcludedNode>,
    pub warnings: Vec<String>,
}

/// Why a node was excluded from generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedNode {
    pub node_id: NodeId,
    pub display_name: String,
    pub reason: String,
}

/// The full compatibility report: included and excluded nodes for a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityReport {
    pub profile: String,
    pub included_node_ids: Vec<NodeId>,
    pub excluded: Vec<ExcludedNode>,
}

impl CompatibilityReport {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.included_node_ids.is_empty() && self.excluded.is_empty()
    }
}

impl std::fmt::Display for CompatibilityReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "profile {}: {} included, {} excluded",
            self.profile,
            self.included_node_ids.len(),
            self.excluded.len()
        )
    }
}

/// Errors produced by the generation pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GenerationError {
    /// Strict mode was requested and one or more nodes were incompatible.
    #[error("generation failed: {0} incompatible node(s)")]
    IncompatibleNodes(CompatibilityReport),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_from_str_roundtrip() {
        assert_eq!(
            GenerationMode::from_str("strict"),
            Ok(GenerationMode::Strict)
        );
        assert_eq!(
            GenerationMode::from_str("lenient"),
            Ok(GenerationMode::Lenient)
        );
        assert!(GenerationMode::from_str("bogus").is_err());
    }

    #[test]
    fn mode_default_is_lenient() {
        assert_eq!(GenerationMode::default(), GenerationMode::Lenient);
    }

    #[test]
    fn mode_serde_lowercase() {
        let s = serde_json::to_string(&GenerationMode::Strict).expect("ser");
        assert_eq!(s, "\"strict\"");
        let m: GenerationMode = serde_json::from_str("\"lenient\"").expect("de");
        assert_eq!(m, GenerationMode::Lenient);
    }

    #[test]
    fn generation_error_display_mentions_incompatible() {
        let report = CompatibilityReport {
            profile: "xray".to_owned(),
            included_node_ids: vec![],
            excluded: vec![ExcludedNode {
                node_id: NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAAAA").expect("ulid"),
                display_name: "n".to_owned(),
                reason: "unsupported protocol".to_owned(),
            }],
        };
        let err = GenerationError::IncompatibleNodes(report);
        let msg = err.to_string();
        assert!(msg.contains("incompatible"), "got: {msg}");
    }
}
