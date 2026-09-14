//! Structured tracing and metrics for Deve Sub.
//!
//! This crate provides the tracing subscriber setup and structured log
//! configuration. See `docs/plan/03-architecture.md` for the observability
//! layer's position in the hexagonal architecture.

#![cfg_attr(test, allow(clippy::expect_used))]

use std::io::IsTerminal;
use thiserror::Error;

/// Errors produced by observability initialization.
#[derive(Debug, Error)]
pub enum ObservabilityError {
    /// The tracing subscriber could not be initialized.
    #[error("failed to initialize tracing subscriber: {0}")]
    Init(String),
}

/// Initialize the global tracing subscriber with structured compact output.
///
/// # Errors
/// Returns [`ObservabilityError`] if the subscriber is already set or
/// the `RUST_LOG` filter is invalid.
pub fn init_tracing() -> Result<(), ObservabilityError> {
    use tracing_subscriber::EnvFilter;

    let filter = match std::env::var("RUST_LOG") {
        Ok(value) => {
            EnvFilter::try_new(value).map_err(|e| ObservabilityError::Init(e.to_string()))?
        }
        Err(std::env::VarError::NotPresent) => EnvFilter::new("info"),
        Err(_) => {
            return Err(ObservabilityError::Init(
                "RUST_LOG is not valid Unicode".into(),
            ));
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .try_init()
        .map_err(|e| ObservabilityError::Init(e.to_string()))
}
