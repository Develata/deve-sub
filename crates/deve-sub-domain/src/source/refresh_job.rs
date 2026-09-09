//! Source refresh job entity (B-15).
//!
//! A `SourceRefreshJob` tracks the lifecycle of a single source refresh
//! attempt: fetch → parse → enrich → reconcile → publish. The job row is
//! persisted so progress is observable, cancellation is durable, and at
//! most one Running job per source exists at any time (the lease).
//!
//! See `docs/plan/milestones/M4-sources-and-node-pool.md` §"Source refresh
//! job model" (B-15).

use deve_sub_kernel::{SourceId, SourceRefreshJobId, Timestamp};

/// Status of a source refresh job.
///
/// Transitions: Pending → Running → (Completed | Failed | Cancelled).
/// The `Pending` state is short-lived — the runner immediately transitions
/// to `Running` after acquiring the lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceRefreshJobStatus {
    /// Created but not yet started by the runner.
    Pending,
    /// The runner is actively refreshing (fetch/parse/enrich/reconcile).
    Running,
    /// The refresh completed successfully; a new snapshot was published
    /// (or a 304 Not-Modified was handled).
    Completed,
    /// The refresh failed (fetch error, parse error, storage error).
    Failed,
    /// Cancelled by the user or a shutdown signal. No snapshot published.
    Cancelled,
}

impl SourceRefreshJobStatus {
    /// Convert to kebab-case string for API serialization.
    #[must_use]
    pub const fn as_kebab(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether this status is terminal (no further transitions).
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// The phase of a refresh job, indicating which step is currently in progress.
///
/// Written to the job row before each phase begins so progress is observable
/// (SRC-002: "创建任务并显示进度").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefreshPhase {
    /// Job created, not yet started.
    Idle,
    /// Fetching the subscription content from the source URL.
    Fetching,
    /// Parsing the fetched content into node entries.
    Parsing,
    /// Enriching entries with GeoIP region detection and applying filters.
    Enriching,
    /// Reconciling parsed entries against the existing node pool.
    Reconciling,
    /// Publishing the new snapshot (atomic: deactivate old + insert new).
    Publishing,
}

impl RefreshPhase {
    /// Stable phase name used in API progress reports.
    #[must_use]
    pub const fn as_kebab(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Fetching => "fetching",
            Self::Parsing => "parsing",
            Self::Enriching => "enriching",
            Self::Reconciling => "reconciling",
            Self::Publishing => "publishing",
        }
    }
}

/// A persistent source refresh job (B-15).
///
/// One row per refresh attempt. The partial UNIQUE index
/// `idx_refresh_jobs_lease` on `(source_id) WHERE status = 'R'` enforces
/// the per-source lease at the DB level: at most one Running job per source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRefreshJob {
    /// Unique identifier (ULID).
    pub id: SourceRefreshJobId,
    /// The source being refreshed.
    pub source_id: SourceId,
    /// Current job status.
    pub status: SourceRefreshJobStatus,
    /// Current phase (progress indicator).
    pub phase: RefreshPhase,
    /// When the job was created, updated on each phase transition as a
    /// heartbeat (P0-10). The scheduler's stale-lease sweep treats a
    /// `started_at` older than `lease_timeout` as evidence the job is dead.
    pub started_at: Timestamp,
    /// When the job reached a terminal status. `None` if still running.
    pub finished_at: Option<Timestamp>,
    /// Error message if status is `Failed`; `None` otherwise.
    pub error_message: Option<String>,
    /// Reconcile counts, populated on success.
    pub new_nodes: u64,
    /// Reconcile counts: duplicate nodes.
    pub duplicate_nodes: u64,
    /// Reconcile counts: reactivated nodes.
    pub reactivated_nodes: u64,
    /// Reconcile counts: missing nodes.
    pub missing_nodes: u64,
    /// Whether the refresh resulted in a 304 Not-Modified response.
    pub not_modified: bool,
}
