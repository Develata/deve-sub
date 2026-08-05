//! Parsing errors for the protocol engine.
//!
//! All variants use `thiserror` for structured error handling. No `anyhow` in
//! public library APIs (see AGENTS.md §"Rust discipline").

use thiserror::Error;

/// Errors produced while parsing a share URI or container format into a
/// canonical [`deve_sub_domain::Node`].
#[derive(Debug, Error)]
pub enum ParseError {
    /// The URI is not parseable by the URL parser.
    #[error("invalid URI: {0}")]
    InvalidUri(String),

    /// The URI scheme is not a recognized proxy protocol.
    #[error("unknown scheme: {0}")]
    UnknownScheme(String),

    /// A required field is missing from the URI.
    #[error("missing required field: {0}")]
    MissingField(&'static str),

    /// A field has an invalid value.
    #[error("invalid field {field}: {value}")]
    InvalidField { field: &'static str, value: String },

    /// The `security` query parameter is not `reality` for a VLESS URI that
    /// was expected to be Reality.
    #[error("expected security=reality, got: {0}")]
    NotReality(String),

    /// The host portion of the URI is missing or invalid.
    #[error("invalid host: {0}")]
    InvalidHost(String),

    /// The port is missing or out of range.
    #[error("invalid port: {0}")]
    InvalidPort(String),

    /// Base64 decoding failed (Shadowsocks userinfo, VMess body).
    #[error("invalid base64: {0}")]
    InvalidBase64(String),

    /// JSON parsing failed (VMess body).
    #[error("invalid JSON: {0}")]
    InvalidJson(String),

    /// A bandwidth value (e.g. Hysteria2 `up`/`down`) could not be parsed.
    #[error("invalid bandwidth: {0}")]
    InvalidBandwidth(String),
}
