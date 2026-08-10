//! Probe domain entities: probe source, latency record, and probe run.
//!
//! A `ProbeSource` is an external monitoring panel (Nezha, DStatus, Komari)
//! configured as a traffic data source. A `LatencyRecord` captures one node
//! latency measurement. A `ProbeRun` is a batch latency probing job. See
//! `docs/plan/milestones/M7-probes-and-detection.md`.

use deve_sub_kernel::{
    LatencyRecordId, NodeId, ProbeRunId, ProbeSourceId, SubscriptionId, Timestamp,
};

/// The kind of external probe panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProbeSourceKind {
    /// Nezha monitoring panel (Bearer PAT auth, cumulative counters).
    Nezha,
    /// DStatus (anonymous, used/limit quota model).
    DStatus,
    /// Komari (anonymous, cumulative counters).
    Komari,
}

impl ProbeSourceKind {
    /// Convert to the single-character discriminator stored in the database.
    #[must_use]
    pub const fn as_db_char(&self) -> &'static str {
        match self {
            Self::Nezha => "N",
            Self::DStatus => "D",
            Self::Komari => "K",
        }
    }

    /// Parse from the single-character database discriminator.
    ///
    /// # Errors
    /// Returns `None` if `c` is not a recognized discriminator.
    #[must_use]
    pub fn from_db_char(c: &str) -> Option<Self> {
        match c {
            "N" => Some(Self::Nezha),
            "D" => Some(Self::DStatus),
            "K" => Some(Self::Komari),
            _ => None,
        }
    }

    /// Convert to kebab-case string for API serialization.
    #[must_use]
    pub const fn as_kebab(&self) -> &'static str {
        match self {
            Self::Nezha => "nezha",
            Self::DStatus => "dstatus",
            Self::Komari => "komari",
        }
    }
}

/// The sync status of a probe source's last traffic sync attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    /// The last sync succeeded.
    Ok,
    /// The last sync failed. The previous traffic data is preserved but marked
    /// stale (PROBE-004).
    Failed(String),
    /// The source has never been synced or the last sync is stale.
    Stale,
}

/// The type of latency probe to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProbeType {
    /// TCP connect to `node.endpoint.host:port`, measure RTT.
    TcpConnect,
    /// QUIC handshake for HY2/TUIC nodes only. Measures handshake RTT. Other
    /// UDP protocols do not get a fake "UDP ping" (spec §98, NODE-014).
    QuicHandshake,
    /// Real proxy request through the node, measure RTT. Most accurate (spec
    /// §94). Ships in Slice 2.
    RealProxy,
}

impl ProbeType {
    /// Convert to the single-character discriminator stored in the database.
    #[must_use]
    pub const fn as_db_char(&self) -> &'static str {
        match self {
            Self::TcpConnect => "T",
            Self::QuicHandshake => "Q",
            Self::RealProxy => "R",
        }
    }

    /// Parse from the single-character database discriminator.
    ///
    /// # Errors
    /// Returns `None` if `c` is not a recognized discriminator.
    #[must_use]
    pub fn from_db_char(c: &str) -> Option<Self> {
        match c {
            "T" => Some(Self::TcpConnect),
            "Q" => Some(Self::QuicHandshake),
            "R" => Some(Self::RealProxy),
            _ => None,
        }
    }

    /// Convert to kebab-case string for API serialization.
    #[must_use]
    pub const fn as_kebab(&self) -> &'static str {
        match self {
            Self::TcpConnect => "tcp_connect",
            Self::QuicHandshake => "quic_handshake",
            Self::RealProxy => "real_proxy",
        }
    }

    /// Parse from kebab-case string.
    ///
    /// # Errors
    /// Returns `None` if `s` is not a recognized probe type.
    #[must_use]
    pub fn from_kebab(s: &str) -> Option<Self> {
        match s {
            "tcp_connect" => Some(Self::TcpConnect),
            "quic_handshake" => Some(Self::QuicHandshake),
            "real_proxy" => Some(Self::RealProxy),
            _ => None,
        }
    }
}

/// Error classification for a latency probe result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    /// Connection refused (TCP RST).
    Refused,
    /// DNS resolution failed.
    DnsFailed,
    /// Connection or handshake timed out.
    Timeout,
    /// TLS handshake failed (certificate, alert, etc.).
    TlsFailed,
    /// QUIC handshake failed (HY2/TUIC only).
    QuicFailed,
    /// The probe succeeded.
    Ok,
}

impl ErrorClass {
    /// Convert to the single-character discriminator stored in the database.
    /// `None` means no error (success); stored as NULL.
    #[must_use]
    pub const fn as_db_char(&self) -> &'static str {
        match self {
            Self::Refused => "R",
            Self::DnsFailed => "D",
            Self::Timeout => "T",
            Self::TlsFailed => "L",
            Self::QuicFailed => "Q",
            Self::Ok => "O",
        }
    }

    /// Parse from the single-character database discriminator.
    ///
    /// # Errors
    /// Returns `None` if `c` is not a recognized discriminator.
    #[must_use]
    pub fn from_db_char(c: &str) -> Option<Self> {
        match c {
            "R" => Some(Self::Refused),
            "D" => Some(Self::DnsFailed),
            "T" => Some(Self::Timeout),
            "L" => Some(Self::TlsFailed),
            "Q" => Some(Self::QuicFailed),
            "O" => Some(Self::Ok),
            _ => None,
        }
    }
}

/// The status of a probe run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProbeRunStatus {
    /// Created but not yet started by the runner.
    Pending,
    /// The runner is actively probing nodes.
    Running,
    /// All probes finished (success or per-node error).
    Completed,
    /// Cancelled by the user (NODE-016). In-flight probes aborted; pending
    /// probes skipped.
    Cancelled,
    /// The run failed (runner crash or unrecoverable error).
    Failed,
}

impl ProbeRunStatus {
    /// Convert to the single-character discriminator stored in the database.
    #[must_use]
    pub const fn as_db_char(&self) -> &'static str {
        match self {
            Self::Pending => "P",
            Self::Running => "R",
            Self::Completed => "C",
            Self::Cancelled => "X",
            Self::Failed => "F",
        }
    }

    /// Parse from the single-character database discriminator.
    ///
    /// # Errors
    /// Returns `None` if `c` is not a recognized discriminator.
    #[must_use]
    pub fn from_db_char(c: &str) -> Option<Self> {
        match c {
            "P" => Some(Self::Pending),
            "R" => Some(Self::Running),
            "C" => Some(Self::Completed),
            "X" => Some(Self::Cancelled),
            "F" => Some(Self::Failed),
            _ => None,
        }
    }

    /// Convert to kebab-case string for API serialization.
    #[must_use]
    pub const fn as_kebab(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    /// Whether this status is terminal (no further transitions).
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

/// The result of a single node latency probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyResult {
    /// The node that was probed.
    pub node_id: NodeId,
    /// Round-trip time in milliseconds. `None` means no response (NODE-014:
    /// no fake latency, node not disabled).
    pub rtt_ms: Option<u32>,
    /// Error classification. `Ok` if `rtt_ms` is `Some`.
    pub error_class: ErrorClass,
}

/// A single latency measurement record, persisted after a probe completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyRecord {
    /// Unique identifier (ULID).
    pub id: LatencyRecordId,
    /// The probe run that produced this record.
    pub run_id: ProbeRunId,
    /// The node that was probed.
    pub node_id: NodeId,
    /// The probe type that produced this record.
    pub probe_type: ProbeType,
    /// Round-trip time in milliseconds. `None` means no response (NODE-014).
    pub rtt_ms: Option<u32>,
    /// Error classification. `Ok` if `rtt_ms` is `Some`.
    pub error_class: ErrorClass,
    /// When the measurement was taken.
    pub measured_at: Timestamp,
}

/// The per-node result within a probe run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRunResult {
    /// The node that was probed.
    pub node_id: NodeId,
    /// Round-trip time in milliseconds. `None` means no response.
    pub rtt_ms: Option<u32>,
    /// Error classification.
    pub error_class: ErrorClass,
    /// Whether this node's probe was skipped (run cancelled before it started).
    pub skipped: bool,
}

/// A batch latency probing job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRun {
    /// Unique identifier (ULID).
    pub id: ProbeRunId,
    /// The probe type to run.
    pub probe_type: ProbeType,
    /// The nodes to probe.
    pub node_ids: Vec<NodeId>,
    /// Current status.
    pub status: ProbeRunStatus,
    /// Per-node results. Populated as probes complete.
    pub results: Vec<ProbeRunResult>,
    /// When the run was created.
    pub created_at: Timestamp,
    /// When the run reached a terminal status. `None` if still pending or
    /// running.
    pub completed_at: Option<Timestamp>,
}

/// An external probe source (Nezha, DStatus, Komari) configured as a traffic
/// data source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSource {
    /// Unique identifier (ULID).
    pub id: ProbeSourceId,
    /// The panel kind.
    pub kind: ProbeSourceKind,
    /// Human-readable name.
    pub name: String,
    /// Panel base URL (e.g. `https://nezha.example.com`).
    pub endpoint_url: String,
    /// Encrypted auth config (API token for Nezha; empty for DStatus/Komari).
    /// Encrypted with XChaCha20-Poly1305 (constitution §157-158).
    pub auth_config: String,
    /// Optional subscription binding for traffic data.
    pub subscription_id: Option<SubscriptionId>,
    /// Whether this source is active.
    pub enabled: bool,
    /// When the last sync was attempted.
    pub last_sync_at: Option<Timestamp>,
    /// The status of the last sync. `None` if never synced.
    pub last_sync_status: Option<SyncStatus>,
    /// Encrypted JSON snapshot of cumulative counters for Nezha/Komari delta
    /// computation. `None` for DStatus (quota model) or on first sync.
    pub last_counter_snapshot: Option<String>,
    /// When the source was created.
    pub created_at: Timestamp,
    /// When the source was last updated.
    pub updated_at: Timestamp,
}

/// A single traffic sample extracted from an external probe panel.
///
/// Normalized across Nezha, DStatus, and Komari to upload/download byte
/// deltas. The `external_server_id` identifies the server within the panel
/// (used by the adapter for cumulative-counter delta tracking).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeTrafficSample {
    /// The server ID in the external panel (Nezha `id`, DStatus node ID,
    /// Komari `uuid`).
    pub external_server_id: String,
    /// Upload bytes in this sample (delta since last sync for cumulative
    /// models; current `used` for quota models).
    pub upload: u64,
    /// Download bytes in this sample.
    pub download: u64,
    /// When the sample was observed.
    pub recorded_at: Timestamp,
}

/// Result of a probe source traffic sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSyncResult {
    /// Traffic samples extracted from the panel.
    pub samples: Vec<ProbeTrafficSample>,
    /// New encrypted counter snapshot for Nezha/Komari cumulative models.
    /// `None` for DStatus (quota model) or if no counter state is needed.
    pub new_counter_snapshot: Option<String>,
}
