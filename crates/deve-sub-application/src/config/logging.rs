//! Configuration for the audit lifecycle; runtime rotation belongs to the host.

use serde::{Deserialize, Serialize};

/// Audit history retention in days; zero explicitly disables automatic expiry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// Default 90 days, enabled range 1–3650. Changes take effect after restart.
    pub audit_retention_days: u32,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            audit_retention_days: 90,
        }
    }
}
