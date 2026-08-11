//! Dashboard DTOs for the `/api/v1/dashboard/latency` and
//! `/api/v1/dashboard/traffic` endpoints.
//!
//! The dashboard surfaces aggregated probe and traffic state for admin
//! observability. Owned by the contract crate per ADR-0004. See
//! `docs/plan/milestones/M7-probes-and-detection.md` §"Traffic aggregation".

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::probe::{ErrorClassDto, ProbeSourceKindDto, ProbeTypeDto, SyncStatusDto};

/// A single latency record in the dashboard latency view.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardLatencyRecordDto {
    /// The node ULID.
    pub node_id: String,
    /// The probe type.
    pub probe_type: ProbeTypeDto,
    /// Round-trip time in milliseconds. `None` = no response (NODE-014).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtt_ms: Option<u32>,
    /// Error classification. `Ok` when the probe succeeded.
    pub error_class: ErrorClassDto,
    /// When the measurement was taken (ISO 8601 UTC).
    pub measured_at: String,
}

/// Response body for `GET /api/v1/dashboard/latency`.
///
/// Returns the most recent latency records across all nodes, newest first.
/// Used by the admin dashboard to surface node health at a glance.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardLatencyResponse {
    /// Recent latency records (up to `limit`, default 50).
    pub records: Vec<DashboardLatencyRecordDto>,
}

/// Per probe-source traffic contribution in the dashboard traffic view.
///
/// Each entry corresponds to one configured probe source. The `upload` and
/// `download` are the sum of all `TrafficRecord` rows attributed to this
/// source (via `source_ref` prefix matching). The `last_sync_status` surfaces
/// staleness so the dashboard can mark stale data (PROBE-004).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardProbeSourceBreakdownDto {
    /// The probe source ULID.
    pub source_id: String,
    /// Panel kind.
    pub kind: ProbeSourceKindDto,
    /// Human-readable name.
    pub name: String,
    /// Whether the source is enabled.
    pub enabled: bool,
    /// Total upload bytes attributed to this source.
    pub upload: u64,
    /// Total download bytes attributed to this source.
    pub download: u64,
    /// When the last sync was attempted (ISO 8601 UTC).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync_at: Option<String>,
    /// Status of the last sync — `Ok`, `Failed { message }`, or `Stale`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync_status: Option<SyncStatusDto>,
}

/// Per source-kind traffic breakdown in the dashboard traffic view.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardSourceKindBreakdownDto {
    /// Source kind (kebab-case): `airport-header`, `manual-correction`, `probe`.
    pub source_kind: String,
    /// Total upload bytes from this source kind.
    pub upload: u64,
    /// Total download bytes from this source kind.
    pub download: u64,
}

/// Response body for `GET /api/v1/dashboard/traffic`.
///
/// Returns the global traffic aggregate across all subscriptions, broken down
/// by source kind and by individual probe source (PROBE-005 traceability).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardTrafficResponse {
    /// Total upload bytes across all subscriptions.
    pub total_upload: u64,
    /// Total download bytes across all subscriptions.
    pub total_download: u64,
    /// Per source-kind breakdown.
    pub by_source_kind: Vec<DashboardSourceKindBreakdownDto>,
    /// Per probe-source breakdown (only `source_kind = probe` traffic).
    pub by_probe_source: Vec<DashboardProbeSourceBreakdownDto>,
}

/// Query parameters for `GET /api/v1/dashboard/latency`.
#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct DashboardLatencyQuery {
    /// Maximum number of records to return (1-200, default 50).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Query parameters for `GET /api/v1/dashboard/traffic`.
#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct DashboardTrafficQuery {
    /// If provided, restrict the aggregate to a single subscription ULID.
    /// `None` (default) aggregates across all subscriptions.
    #[serde(default)]
    pub subscription_id: Option<String>,
}
