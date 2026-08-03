//! Health endpoint DTOs for `/health/live` and `/health/ready`.
//!
//! These DTOs are the wire format for health probes. They are owned by the
//! contract crate per ADR-0004: DTOs and `ToSchema` derives live here, not
//! in the API crate.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Overall health status reported by health probes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatusDto {
    /// Service is alive and ready.
    Healthy,
    /// Service is alive but not ready.
    Degraded,
    /// Service is not responding correctly.
    Unhealthy,
}

/// Response body for `GET /health/live`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthLiveResponse {
    /// Overall health status.
    pub status: HealthStatusDto,
    /// Product display name.
    pub product_name: String,
    /// Software version.
    pub version: String,
}

/// Response body for `GET /health/ready`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthReadyResponse {
    /// Overall health status.
    pub status: HealthStatusDto,
    /// Product display name.
    pub product_name: String,
    /// Software version.
    pub version: String,
}
