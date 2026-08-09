//! Traffic DTOs for the `/api/v1/subscriptions/{id}/traffic` endpoints.
//!
//! These DTOs are the wire format for subscription traffic accounting and
//! manual correction. They are owned by the contract crate per ADR-0004.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Per-source-kind breakdown entry in a [`TrafficSummaryResponse`].
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TrafficSourceBreakdownDto {
    /// Source kind (kebab-case): `airport-header`, `manual-correction`, `probe`.
    pub source_kind: String,
    /// Upload bytes from this source kind.
    pub upload: u64,
    /// Download bytes from this source kind.
    pub download: u64,
}

/// Response body for `GET /api/v1/subscriptions/{id}/traffic`.
///
/// Returns the aggregated consumed traffic for a subscription, broken down by
/// source kind. The `total` is `upload + download`. Used by the admin traffic
/// dashboard and to verify quota enforcement state (OUT-010/OUT-011).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TrafficSummaryResponse {
    /// The subscription ULID.
    pub subscription_id: String,
    /// Total upload bytes across all source kinds.
    pub upload: u64,
    /// Total download bytes across all source kinds.
    pub download: u64,
    /// Total consumed traffic (`upload + download`).
    pub total: u64,
    /// Per-source-kind breakdown.
    pub by_source: Vec<TrafficSourceBreakdownDto>,
}

/// Request body for `POST /api/v1/subscriptions/{id}/traffic-correction`.
///
/// Records a manual traffic correction (admin escape hatch for drifted totals).
/// The correction is appended like any other record; aggregation is sum-based.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ManualCorrectionRequest {
    /// Upload bytes to record.
    pub upload: u64,
    /// Download bytes to record.
    pub download: u64,
    /// Admin note explaining the correction (max 512 chars).
    pub note: String,
}

/// Response body for `POST /api/v1/subscriptions/{id}/traffic-correction`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ManualCorrectionResponse {
    /// The traffic record ULID.
    pub record_id: String,
    /// The subscription ULID.
    pub subscription_id: String,
    /// Source kind (`manual-correction`).
    pub source_kind: String,
    /// Upload bytes recorded.
    pub upload: u64,
    /// Download bytes recorded.
    pub download: u64,
    /// When the record was created (ISO 8601 UTC).
    pub recorded_at: String,
    /// The admin note / source reference.
    pub source_ref: String,
}
