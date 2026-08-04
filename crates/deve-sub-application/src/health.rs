//! Health check query and port.
//!
//! Implements the health read model for `/health/live` and `/health/ready`
//! endpoints. Defines [`DbHealthPort`] for database connectivity checks,
//! keeping the delivery layer decoupled from the storage adapter.
//! See `docs/plan/milestones/M1-infrastructure.md`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use deve_sub_contract::HealthStatusDto as HealthStatus;

/// Errors produced by health check operations.
#[derive(Debug, thiserror::Error)]
pub enum HealthError {
    /// The database connectivity check failed.
    #[error("database health check failed: {0}")]
    Database(String),
}

/// Port for database health checks.
///
/// The storage adapter implements this trait. The delivery layer calls it
/// through the application layer, not directly, preserving the hexagonal
/// dependency direction (delivery → application → port ← adapter).
#[async_trait]
pub trait DbHealthPort: Send + Sync {
    /// Check database connectivity. Returns `Ok(())` if the database is
    /// reachable.
    ///
    /// # Errors
    /// Returns [`HealthError`] if the database is unreachable.
    async fn check(&self) -> Result<(), HealthError>;
}

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
