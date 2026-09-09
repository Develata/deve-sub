//! Health endpoint DTOs for `/health/live` and `/health/ready`.
//!
//! These DTOs are the wire format for health probes. They are owned by the
//! contract crate per ADR-0004: DTOs and `ToSchema` derives live here, not
//! in the API crate.

use serde::{Deserialize, Serialize};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Overall health status reported by health probes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
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
///
/// Deliberately a distinct type from [`HealthReadyResponse`] even though the
/// fields are identical today: liveness and readiness probes may diverge
/// (e.g. readiness could add per-check details). Keeping them separate
/// preserves independent evolution without a wire-format break.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct HealthLiveResponse {
    /// Overall health status.
    pub status: HealthStatusDto,
    /// Product display name.
    pub product_name: String,
    /// Software version.
    pub version: String,
}

/// Response body for `GET /health/ready`. See [`HealthLiveResponse`] for why
/// this is a separate type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct HealthReadyResponse {
    /// Overall health status.
    pub status: HealthStatusDto,
    /// Product display name.
    pub product_name: String,
    /// Software version.
    pub version: String,
}
