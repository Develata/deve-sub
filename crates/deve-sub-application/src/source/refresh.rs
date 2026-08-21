//! Source refresh job runner (B-15): fetch → parse → enrich → reconcile →
//! publish with persistent job state, progress tracking, and cancellation.
//!
//! This module wraps the core refresh logic with job lifecycle management:
//! - Creates a job row (Pending), transitions to Running (acquiring the
//!   per-source lease via the DB partial unique index).
//! - Updates the job's `phase` before each step so progress is observable
//!   (SRC-002: "创建任务并显示进度").
//! - Checks the cancel flag before each phase (SRC-009: "取消后
//!   不发布半成品"). No check after reconcile — once the snapshot is
//!   committed, the refresh must proceed to Completed (P0-09).
//! - Writes a terminal status (Completed / Failed / Cancelled) on every exit
//!   path so no job is left in Pending or Running.
//! - The snapshot publish (deactivate old + insert new) remains the final
//!   atomic step — a cancelled refresh never publishes a half-built snapshot.

use std::sync::atomic::{AtomicBool, Ordering};

use deve_sub_domain::{
    NodePoolRepository, ReconcileInput, ReconcileResult, RefreshPhase, Source, SourceError,
    SourceRefreshJob, SourceRefreshJobRepository, SourceRefreshJobStatus, SourceRepository,
    SourceSnapshot, SourceSnapshotRepository,
};
use deve_sub_kernel::{SourceId, SourceRefreshJobId, SourceSnapshotId, Timestamp};

use super::error::SourceAppError;
use super::fetcher::{FetchResult, SubscriptionFetcher};
use super::filter::{apply_protocol_filter, apply_region_filter};
use super::geoip::{GeoIpPort, enrich_regions};
use super::parse::parse_content;

/// Result of a successful source refresh.
#[derive(Debug, Clone)]
pub struct RefreshResult {
    /// The snapshot created by this refresh.
    pub snapshot: SourceSnapshot,
    /// Reconciliation counts from the node pool update.
    pub reconcile: ReconcileResult,
    /// Whether the fetch returned 304 Not Modified. When `true`, no new
    /// snapshot was created and `snapshot` refers to the previously active
    /// one.
    pub not_modified: bool,
}

/// Dependencies for a refresh job run, grouped to keep the runner function
/// under the clippy argument limit.
pub struct RefreshDeps<'a> {
    pub source_repo: &'a dyn SourceRepository,
    pub snapshot_repo: &'a dyn SourceSnapshotRepository,
    pub pool_repo: &'a dyn NodePoolRepository,
    pub job_repo: &'a dyn SourceRefreshJobRepository,
    pub fetcher: &'a dyn SubscriptionFetcher,
    pub geoip: &'a dyn GeoIpPort,
}

/// Start a new refresh job for `source_id`.
///
/// Creates a job row in `Pending` status, then transitions it to `Running`.
/// Returns the job ID on success. Returns
/// [`SourceAppError::RefreshInProgress`] if a Running job already exists for
/// this source (the lease is held).
pub async fn start_refresh_job(
    deps: &RefreshDeps<'_>,
    source_id: SourceId,
) -> Result<SourceRefreshJobId, SourceAppError> {
    // WHY: check source existence before creating the job row so the client
    // gets an immediate 404 instead of polling a job that will fail.
    if deps
        .source_repo
        .find_by_id(source_id)
        .await
        .map_err(map_source_error)?
        .is_none()
    {
        return Err(SourceAppError::SourceNotFound);
    }

    let job = SourceRefreshJob {
        id: SourceRefreshJobId::new(),
        source_id,
        status: SourceRefreshJobStatus::Pending,
        phase: RefreshPhase::Idle,
        started_at: Timestamp::now(),
        finished_at: None,
        error_message: None,
        new_nodes: 0,
        duplicate_nodes: 0,
        reactivated_nodes: 0,
        missing_nodes: 0,
        not_modified: false,
    };
    deps.job_repo.create(&job).await.map_err(map_lease_error)?;
    deps.job_repo
        .mark_running(job.id)
        .await
        .map_err(map_lease_error)?;
    Ok(job.id)
}

/// Execute a refresh job to completion. The job must already be in `Running`
/// status (created via [`start_refresh_job`]).
///
/// Checks `cancelled` before each phase. On cancel, writes
/// `Cancelled` terminal status and returns [`SourceAppError::Cancelled`].
/// No cancel check after the reconcile commit — once the snapshot is
/// published, the job must be marked Completed (P0-09).
/// On any other error, writes `Failed` terminal status with the error
/// message and returns the error.
///
/// # Errors
/// - [`SourceAppError::SourceNotFound`] — source does not exist.
/// - [`SourceAppError::Cancelled`] — cancelled by the caller.
/// - [`SourceAppError::Fetch`] — fetch failed.
/// - [`SourceAppError::Parse`] — parse failed.
/// - [`SourceAppError::ZeroNodes`] — zero valid nodes with existing snapshot.
/// - [`SourceAppError::Source`] — storage or reconciliation error.
pub async fn execute_refresh_job(
    deps: &RefreshDeps<'_>,
    job_id: SourceRefreshJobId,
    source_id: SourceId,
    cancelled: &AtomicBool,
) -> Result<RefreshResult, SourceAppError> {
    let result = execute_refresh_inner(deps, job_id, source_id, cancelled).await;

    match &result {
        Ok(r) => {
            let _ = deps
                .job_repo
                .mark_completed(
                    job_id,
                    r.reconcile.new_nodes,
                    r.reconcile.duplicate_nodes,
                    r.reconcile.reactivated_nodes,
                    r.reconcile.missing_nodes,
                    r.not_modified,
                )
                .await;
        }
        Err(SourceAppError::Cancelled) => {
            let _ = deps.job_repo.mark_cancelled(job_id).await;
        }
        Err(e) => {
            let _ = deps.job_repo.mark_failed(job_id, &e.to_string()).await;
        }
    }
    result
}

async fn execute_refresh_inner(
    deps: &RefreshDeps<'_>,
    job_id: SourceRefreshJobId,
    source_id: SourceId,
    cancelled: &AtomicBool,
) -> Result<RefreshResult, SourceAppError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(SourceAppError::Cancelled);
    }

    let source = deps
        .source_repo
        .find_by_id(source_id)
        .await
        .map_err(map_source_error)?
        .ok_or(SourceAppError::SourceNotFound)?;

    let active = deps
        .snapshot_repo
        .find_active(source_id)
        .await
        .map_err(map_source_error)?;
    let etag = active.as_ref().and_then(|s| s.etag.clone());

    // ── Phase: Fetching ──
    deps.job_repo
        .update_phase(job_id, RefreshPhase::Fetching)
        .await
        .map_err(SourceAppError::Source)?;

    let fetch = match deps.fetcher.fetch(&source.url, etag.as_deref()).await {
        Ok(f) => f,
        Err(e) => {
            disable_on_failure(deps.source_repo, &source).await;
            return Err(e.into());
        }
    };

    if cancelled.load(Ordering::Relaxed) {
        return Err(SourceAppError::Cancelled);
    }

    // ── 304 Not Modified ──
    if let FetchResult::NotModified = fetch {
        let snapshot = active.ok_or(SourceAppError::Source(SourceError::Storage(
            "server returned 304 but no active snapshot exists".to_owned(),
        )))?;
        return Ok(RefreshResult {
            snapshot,
            reconcile: ReconcileResult::default(),
            not_modified: true,
        });
    }

    let (body, resp_etag, content_type) = match fetch {
        FetchResult::Ok {
            body,
            etag,
            content_type,
        } => (body, etag, content_type),
        FetchResult::NotModified => {
            // WHY: the NotModified arm above returns early; this arm exists
            // only to satisfy the exhaustive match and can never execute.
            return Err(SourceAppError::Source(SourceError::Storage(
                "unreachable: NotModified after 304 check".to_owned(),
            )));
        }
    };

    // ── Phase: Parsing ──
    deps.job_repo
        .update_phase(job_id, RefreshPhase::Parsing)
        .await
        .map_err(SourceAppError::Source)?;

    let mut entries = match parse_content(source.source_type, content_type.as_deref(), &body) {
        Ok(e) => e,
        Err(e) => {
            disable_on_failure(deps.source_repo, &source).await;
            return Err(e.into());
        }
    };

    if cancelled.load(Ordering::Relaxed) {
        return Err(SourceAppError::Cancelled);
    }

    // WHY: SRC-006 — zero valid nodes with existing snapshot preserves the
    // old snapshot instead of marking all nodes as missing. This is the
    // unified zero-node strategy: error out, do not publish.
    let valid_after_parse = entries.iter().filter(|e| e.node.is_some()).count();
    if valid_after_parse == 0 && active.is_some() {
        return Err(SourceAppError::ZeroNodes);
    }

    // ── Phase: Enriching ──
    deps.job_repo
        .update_phase(job_id, RefreshPhase::Enriching)
        .await
        .map_err(SourceAppError::Source)?;

    // WHY: SRC-010 phase 1 — protocol filter before GeoIP to skip lookups.
    if let Some(ref rules) = source.filter_rules {
        apply_protocol_filter(&mut entries, rules);
    }
    enrich_regions(&mut entries, deps.geoip).await;
    // WHY: SRC-010 phase 2 — region filter after GeoIP enrichment.
    if let Some(ref rules) = source.filter_rules {
        apply_region_filter(&mut entries, rules);
    }

    if cancelled.load(Ordering::Relaxed) {
        return Err(SourceAppError::Cancelled);
    }

    // ── Phase: Reconciling ──
    deps.job_repo
        .update_phase(job_id, RefreshPhase::Reconciling)
        .await
        .map_err(SourceAppError::Source)?;

    let new_version = active.as_ref().map(|s| s.version + 1).unwrap_or(1);
    let node_count =
        u64::try_from(entries.iter().filter(|e| e.node.is_some()).count()).map_err(|_| {
            SourceAppError::Source(SourceError::Storage("node count overflow".to_owned()))
        })?;

    let snapshot = SourceSnapshot {
        id: SourceSnapshotId::new(),
        source_id,
        version: new_version,
        fetched_at: Timestamp::now(),
        etag: resp_etag,
        node_count,
        is_active: true,
    };

    let input = ReconcileInput {
        source_id,
        snapshot: &snapshot,
        entries: &entries,
    };
    let reconcile = deps
        .pool_repo
        .reconcile(input)
        .await
        .map_err(map_source_error)?;

    // ── Phase: Publishing ──
    // WHY (P0-09): no cancel check after reconcile. The reconcile call
    // atomically commits the new snapshot and node pool changes. If a
    // cancel signal arrives during or after the commit, the data is
    // already published — returning Cancelled would mark the job as
    // Cancelled while the refresh was actually applied, a lie. Once
    // reconcile has committed, the refresh must proceed to Completed.
    // Cancel checks before each phase (lines above) are sufficient to
    // prevent unwanted refreshes; a cancel that arrives after the last
    // pre-reconcile check (line 257) is too late to stop the commit.
    deps.job_repo
        .update_phase(job_id, RefreshPhase::Publishing)
        .await
        .map_err(SourceAppError::Source)?;

    Ok(RefreshResult {
        snapshot,
        reconcile,
        not_modified: false,
    })
}

/// Cancel a refresh job by ID. Best-effort — the job may have already
/// completed. Sets the `cancelled` flag so the runner's cancel checks abort
/// at the next phase boundary.
pub fn signal_cancel(cancelled: &AtomicBool) {
    cancelled.store(true, Ordering::Relaxed);
}

/// Best-effort disable a source after a refresh failure when `keep_on_fail`
/// is false (unchanged from the original implementation).
async fn disable_on_failure(repo: &dyn SourceRepository, source: &Source) {
    if !source.keep_on_fail && source.enabled {
        let mut disabled = source.clone();
        disabled.enabled = false;
        if let Err(e) = repo.update(&disabled).await {
            tracing::warn!(error = %e, "failed to disable source after refresh failure");
        }
    }
}

fn map_source_error(e: SourceError) -> SourceAppError {
    match e {
        SourceError::NameExists => SourceAppError::NameExists,
        other => SourceAppError::Source(other),
    }
}

fn map_lease_error(e: SourceError) -> SourceAppError {
    match e {
        SourceError::RefreshInProgress(id) => SourceAppError::RefreshInProgress(id),
        other => SourceAppError::Source(other),
    }
}

/// Mark all source refresh jobs left in `Pending` or `Running` status as
/// `Failed` (crash recovery on startup, constraint #20). Returns the count
/// of recovered jobs.
///
/// Mirrors [`probe::recover_crashed_runs`](crate::probe::recover_crashed_runs)
/// for the source refresh job lease: a process crash mid-refresh leaves the
/// job row in `Running`, and the partial UNIQUE index
/// `idx_refresh_jobs_lease` then blocks all future refreshes for that
/// source. Calling this on startup releases those orphaned leases (P0-10).
///
/// # Errors
/// Returns [`SourceAppError::Source`] if the storage update fails.
pub async fn recover_crashed_refresh_jobs(
    job_repo: &dyn SourceRefreshJobRepository,
) -> Result<u64, SourceAppError> {
    Ok(job_repo.recover_crashed_jobs().await?)
}

/// Mark `Running` refresh jobs whose `started_at` is older than `cutoff` as
/// `Failed` (scheduler tick lease sweep). Returns the count of recovered
/// jobs.
///
/// A refresh that exceeds the lease timeout is presumed dead — the runner
/// task was killed, panicked without unwinding, or the host lost power.
/// This bounded age-based check runs at each scheduler tick so a hung job
/// does not hold the lease for the entire uptime (P0-10).
///
/// # Errors
/// Returns [`SourceAppError::Source`] if the storage update fails.
pub async fn recover_stale_refresh_jobs(
    job_repo: &dyn SourceRefreshJobRepository,
    cutoff: Timestamp,
    reason: &str,
) -> Result<u64, SourceAppError> {
    Ok(job_repo.recover_stale_jobs(cutoff, reason).await?)
}
