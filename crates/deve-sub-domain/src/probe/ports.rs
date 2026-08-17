//! Probe domain port traits: storage boundaries and the latency probe port.
//!
//! Storage ports (`ProbeSourceRepository`, `LatencyRecordRepository`,
//! `ProbeRunRepository`) are implemented by the SQLite adapter. The
//! `LatencyProbe` port is implemented by the probe adapters (TCP connect,
//! QUIC handshake, real proxy).

use async_trait::async_trait;
use deve_sub_kernel::{NodeId, ProbeRunId, ProbeSourceId, Timestamp};

use crate::node::Node;
use crate::probe::entity::{
    LatencyRecord, LatencyResult, ProbeRun, ProbeRunStatus, ProbeSource, ProbeSourceKind,
    ProbeSyncResult,
};
use crate::probe::error::ProbeError;

/// Storage boundary for probe source aggregates.
#[async_trait]
pub trait ProbeSourceRepository: Send + Sync {
    /// Create a new probe source. Returns
    /// [`ProbeError::NameExists`] if the name is already taken.
    async fn create(&self, source: &ProbeSource) -> Result<(), ProbeError>;

    /// Find a probe source by ID.
    async fn find_by_id(&self, id: ProbeSourceId) -> Result<Option<ProbeSource>, ProbeError>;

    /// List probe sources with cursor pagination. Returns up to `limit`
    /// sources whose ULID is strictly greater than `cursor`, ordered by `id`.
    /// If `kind` is `Some`, filters by kind.
    async fn list(
        &self,
        cursor: Option<ProbeSourceId>,
        limit: u32,
        kind: Option<ProbeSourceKind>,
    ) -> Result<Vec<ProbeSource>, ProbeError>;

    /// Update a probe source's mutable fields. Returns
    /// [`ProbeError::SourceNotFound`] if the source does not exist, or
    /// [`ProbeError::NameExists`] on name collision.
    async fn update(&self, source: &ProbeSource) -> Result<(), ProbeError>;

    /// Delete a probe source.
    async fn delete(&self, id: ProbeSourceId) -> Result<(), ProbeError>;
}

/// Storage boundary for latency records.
#[async_trait]
pub trait LatencyRecordRepository: Send + Sync {
    /// Insert a latency record.
    async fn create(&self, record: &LatencyRecord) -> Result<(), ProbeError>;

    /// List recent latency records for a node, ordered by `measured_at` desc.
    /// Returns up to `limit` records.
    async fn list_for_node(
        &self,
        node_id: NodeId,
        limit: u32,
    ) -> Result<Vec<LatencyRecord>, ProbeError>;

    /// List recent latency records across all nodes, ordered by `measured_at`
    /// desc. Returns up to `limit` records. Used by the dashboard latency view.
    async fn list_recent(&self, limit: u32) -> Result<Vec<LatencyRecord>, ProbeError>;

    /// Delete all latency records for a probe run. Called on run cancellation
    /// to clean partial results if needed.
    async fn delete_for_run(&self, run_id: ProbeRunId) -> Result<(), ProbeError>;
}

/// Storage boundary for probe run aggregates.
#[async_trait]
pub trait ProbeRunRepository: Send + Sync {
    /// Create a new probe run.
    async fn create(&self, run: &ProbeRun) -> Result<(), ProbeError>;

    /// Find a probe run by ID.
    async fn find_by_id(&self, id: ProbeRunId) -> Result<Option<ProbeRun>, ProbeError>;

    /// Update a probe run's status and results. Used by the runner as probes
    /// complete.
    async fn update_status(
        &self,
        id: ProbeRunId,
        status: ProbeRunStatus,
        results: &[crate::probe::entity::ProbeRunResult],
        completed_at: Option<Timestamp>,
    ) -> Result<(), ProbeError>;

    /// Persist results and completion timestamp WITHOUT changing status.
    ///
    /// WHY: when a concurrent cancel wins the status race (W-F), the runner's
    /// `update_status` hits the terminal guard and returns
    /// `RunAlreadyTerminal`. The runner still has collected diagnostic
    /// results that should be visible to the user, so it calls this method to
    /// persist them on the already-terminal row.
    async fn update_results(
        &self,
        id: ProbeRunId,
        results: &[crate::probe::entity::ProbeRunResult],
        completed_at: Option<Timestamp>,
    ) -> Result<(), ProbeError>;

    /// Mark any runs in `Running` status as `Failed` (crash recovery on
    /// startup). Returns the count of recovered runs.
    async fn recover_crashed_runs(&self) -> Result<u64, ProbeError>;

    /// Delete a probe run and its results.
    async fn delete(&self, id: ProbeRunId) -> Result<(), ProbeError>;
}

/// Port trait for a single-node latency probe. Implemented by the TCP
/// connect, QUIC handshake, and real proxy probe adapters.
#[async_trait]
pub trait LatencyProbe: Send + Sync {
    /// Probe a single node and return the latency result.
    ///
    /// Implementations must:
    /// - respect the `timeout` parameter (abort after the deadline);
    /// - classify errors into [`ErrorClass`];
    /// - return `rtt_ms = None` + `error_class = Timeout` for no response
    ///   (NODE-014: no fake latency, no auto-kill).
    async fn probe(&self, node: &Node, timeout: std::time::Duration) -> LatencyResult;
}

/// Port trait for an external probe panel traffic sync adapter.
///
/// Each panel (Nezha, DStatus, Komari) implements this trait. The application
/// `sync_probe_traffic` command calls the adapter, maps samples to
/// `TrafficRecord` rows (source_kind = Probe), and persists the new counter
/// snapshot. See `docs/plan/milestones/M7-probes-and-detection.md`
/// §"Probe source adapter Port".
#[async_trait]
pub trait ProbeSourceAdapter: Send + Sync {
    /// Sync traffic data from the external panel.
    ///
    /// Implementations must:
    /// - decrypt `auth_config` and `last_counter_snapshot` as needed;
    /// - call the panel API;
    /// - compute upload/download deltas (cumulative models) or current usage
    ///   (quota models);
    /// - encrypt the new counter snapshot for persistence.
    async fn sync_traffic(&self, source: &ProbeSource) -> Result<ProbeSyncResult, ProbeError>;
}
