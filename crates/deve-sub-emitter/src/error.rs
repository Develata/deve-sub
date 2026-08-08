//! Emission errors for the output format engine.
//!
//! All variants use `thiserror` for structured error handling. No `anyhow` in
//! public library APIs (see AGENTS.md §"Rust discipline").

use thiserror::Error;

/// Errors produced while emitting a canonical [`deve_sub_domain::Node`] to a
/// share URI or target format.
#[derive(Debug, Error)]
pub enum EmitError {
    /// The node's protocol has no URI emitter (e.g. `Unsupported` nodes).
    #[error("no URI emitter for protocol: {0}")]
    NoEmitter(String),

    /// A required field is missing from the node for URI emission.
    #[error("missing required field for emission: {0}")]
    MissingField(&'static str),

    /// A field has an invalid value that cannot be emitted.
    #[error("invalid field {field}: {value}")]
    InvalidField { field: &'static str, value: String },

    /// A container-format encoder failed (e.g. JSON/YAML serialization).
    #[error("encode error: {0}")]
    Encode(String),
}
