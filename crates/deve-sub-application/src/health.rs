//! Health check query.
//!
//! Implements the health read model for `/health/live` and `/health/ready`
//! endpoints. See `docs/plan/milestones/M1-infrastructure.md`.

use serde::{Deserialize, Serialize};

pub use deve_sub_contract::HealthStatusDto as HealthStatus;

/// Health view returned by the health query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthView {
    /// Overall status.
    pub status: HealthStatus,
    /// Product name.
    pub product_name: String,
    /// Software version.
    pub version: String,
}

impl HealthView {
    /// Create a live health view (liveness probe).
    #[must_use]
    pub fn live(product_name: &str, version: &str) -> Self {
        Self {
            status: HealthStatus::Healthy,
            product_name: product_name.to_owned(),
            version: version.to_owned(),
        }
    }

    /// Create a ready health view with the given status.
    #[must_use]
    pub fn ready(status: HealthStatus, product_name: &str, version: &str) -> Self {
        Self {
            status,
            product_name: product_name.to_owned(),
            version: version.to_owned(),
        }
    }
}
