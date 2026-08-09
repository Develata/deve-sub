//! Probe DTOs for the `/api/v1/probe-sources`, `/api/v1/probe-runs`, and
//! `/api/v1/nodes/{id}/latency` endpoints.
//!
//! Owned by the contract crate per ADR-0004: DTOs and `ToSchema` derives live
//! here, not in the API crate. See
//! `docs/plan/milestones/M7-probes-and-detection.md` §"Server".

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The kind of external probe panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeSourceKindDto {
    /// Nezha monitoring panel (Bearer PAT auth, cumulative counters).
    Nezha,
    /// DStatus (anonymous, used/limit quota model).
    Dstatus,
    /// Komari (anonymous, cumulative counters).
    Komari,
}

/// The type of latency probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeTypeDto {
    /// TCP connect RTT.
    TcpConnect,
    /// QUIC handshake RTT (HY2/TUIC only).
    QuicHandshake,
    /// Real proxy request RTT.
    RealProxy,
}

/// The status of a probe run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeRunStatusDto {
    /// Created but not yet started.
    Pending,
    /// Actively probing.
    Running,
    /// All probes finished.
    Completed,
    /// Cancelled by the user.
    Cancelled,
    /// The run failed.
    Failed,
}

/// The error classification of a latency result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClassDto {
    /// Connection refused.
    Refused,
    /// DNS resolution failed.
    DnsFailed,
    /// Timed out.
    Timeout,
    /// TLS handshake failed.
    TlsFailed,
    /// QUIC handshake failed.
    QuicFailed,
    /// Success.
    Ok,
}

/// Sync status of a probe source's last traffic sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case", untagged)]
pub enum SyncStatusDto {
    /// The last sync succeeded.
    Ok,
    /// The last sync failed with the given message.
    Failed { message: String },
    /// Never synced or stale.
    Stale,
}

/// Probe source information returned by probe source endpoints.
///
/// Never includes the raw `auth_config` ciphertext in responses.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProbeSourceDto {
    /// ULID identifier.
    pub id: String,
    /// Panel kind.
    pub kind: ProbeSourceKindDto,
    /// Human-readable name.
    pub name: String,
    /// Panel base URL.
    pub endpoint_url: String,
    /// Whether an auth credential is configured (the credential itself is not
    /// returned).
    pub has_auth: bool,
    /// Bound subscription ULID, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<String>,
    /// Whether the source is active.
    pub enabled: bool,
    /// When the last sync was attempted (ISO 8601 UTC).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync_at: Option<String>,
    /// Status of the last sync.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync_status: Option<SyncStatusDto>,
    /// Creation time (ISO 8601 UTC).
    pub created_at: String,
    /// Last update time (ISO 8601 UTC).
    pub updated_at: String,
}

/// Request body for `POST /api/v1/probe-sources`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateProbeSourceRequest {
    /// Panel kind.
    pub kind: ProbeSourceKindDto,
    /// Human-readable name.
    pub name: String,
    /// Panel base URL.
    pub endpoint_url: String,
    /// Auth config (API token for Nezha; empty for DStatus/Komari). Encrypted
    /// at rest with XChaCha20-Poly1305.
    #[serde(default)]
    pub auth_config: String,
    /// Bound subscription ULID, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<String>,
}

/// Request body for `PUT /api/v1/probe-sources/{id}`. Only provided fields are
/// mutated.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateProbeSourceRequest {
    /// Human-readable name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Panel base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_url: Option<String>,
    /// Auth config. Replaces the existing credential.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_config: Option<String>,
    /// Bound subscription ULID. `None` clears the binding; omitting the field
    /// leaves it unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<Option<String>>,
    /// Whether the source is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Response body for `POST /api/v1/probe-sources` and
/// `PUT /api/v1/probe-sources/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProbeSourceResponse {
    /// The probe source.
    pub source: ProbeSourceDto,
}

/// Response body for `GET /api/v1/probe-sources` (cursor-paginated).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListProbeSourcesResponse {
    /// Sources in the current page.
    pub sources: Vec<ProbeSourceDto>,
    /// Cursor for the next page (`None` if no more results).
    pub next_cursor: Option<String>,
}

/// Request body for `POST /api/v1/probe-runs`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateProbeRunRequest {
    /// The probe type to run.
    pub probe_type: ProbeTypeDto,
    /// Node ULIDs to probe.
    pub node_ids: Vec<String>,
}

/// Per-node result within a probe run.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProbeRunResultDto {
    /// Node ULID.
    pub node_id: String,
    /// Round-trip time in milliseconds. `None` means no response (NODE-014).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtt_ms: Option<u32>,
    /// Error classification.
    pub error_class: ErrorClassDto,
    /// Whether this node's probe was skipped (run cancelled before it started).
    #[serde(default)]
    pub skipped: bool,
}

/// Probe run information returned by probe run endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProbeRunDto {
    /// ULID identifier.
    pub id: String,
    /// The probe type.
    pub probe_type: ProbeTypeDto,
    /// Node ULIDs in the run.
    pub node_ids: Vec<String>,
    /// Current status.
    pub status: ProbeRunStatusDto,
    /// Per-node results. Populated as probes complete.
    pub results: Vec<ProbeRunResultDto>,
    /// Creation time (ISO 8601 UTC).
    pub created_at: String,
    /// Terminal time (ISO 8601 UTC). `None` if still pending or running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

/// Response body for `POST /api/v1/probe-runs` and `GET /api/v1/probe-runs/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProbeRunResponse {
    /// The probe run.
    pub run: ProbeRunDto,
}

/// A single latency measurement record.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LatencyRecordDto {
    /// ULID identifier.
    pub id: String,
    /// The probe run that produced this record.
    pub run_id: String,
    /// Node ULID.
    pub node_id: String,
    /// The probe type.
    pub probe_type: ProbeTypeDto,
    /// Round-trip time in milliseconds. `None` means no response (NODE-014).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtt_ms: Option<u32>,
    /// Error classification.
    pub error_class: ErrorClassDto,
    /// When the measurement was taken (ISO 8601 UTC).
    pub measured_at: String,
}

/// Response body for `GET /api/v1/nodes/{id}/latency`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListLatencyRecordsResponse {
    /// Latency records, newest first.
    pub records: Vec<LatencyRecordDto>,
}
