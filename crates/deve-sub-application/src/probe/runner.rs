//! Probe runner: batch latency probing with semaphore-bounded concurrency,
//! observable progress, and cancellation support (constraint #20).
//!
//! The runner is a built-in background job, not a separate service. It is
//! observable (ProbeRun status + per-node results queryable), cancellable
//! (cancellation flag, NODE-016), and safely shut down on server stop. See
//! `docs/plan/milestones/M7-probes-and-detection.md` §"Probe runner".

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use deve_sub_domain::{
    ErrorClass, LatencyProbe, LatencyRecord, LatencyRecordRepository, LatencyResult,
    NodePoolRepository, ProbeError, ProbeRunRepository, ProbeRunResult, ProbeRunStatus, ProbeType,
};
use deve_sub_kernel::{LatencyRecordId, NodeId, ProbeRunId, Timestamp};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Default per-probe timeout (5 seconds).
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Default concurrency limit (32 concurrent probes).
pub const DEFAULT_CONCURRENCY: usize = 32;

/// Configuration for a probe runner execution.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Per-probe timeout.
    pub timeout: Duration,
    /// Maximum concurrent probes.
    pub concurrency: usize,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_PROBE_TIMEOUT,
            concurrency: DEFAULT_CONCURRENCY,
        }
    }
}

/// Dependencies injected into a probe runner execution: the probe adapter and
/// the repositories the runner reads from / writes to. Grouping them keeps
/// `execute_probe_run` under the clippy argument limit.
#[derive(Clone)]
pub struct RunnerDeps {
    /// The latency probe adapter (TCP connect, QUIC handshake, or real proxy).
    pub probe: Arc<dyn LatencyProbe>,
    /// Node pool repository — used to fetch node endpoints by ID.
    pub pool_repo: Arc<dyn NodePoolRepository>,
    /// Probe run repository — used to read and update run status/results.
    pub run_repo: Arc<dyn ProbeRunRepository>,
    /// Latency record repository — persists per-node latency measurements.
    pub latency_repo: Arc<dyn LatencyRecordRepository>,
}

/// Execute a probe run: probe each node with semaphore-bounded concurrency,
/// update the run status and results as probes complete, and respect
/// cancellation.
///
/// # Cancellation
/// If `cancelled` is set to `true` (by `cancel_probe_run`), in-flight probes
/// are allowed to finish their current attempt (bounded by `timeout`), and
/// pending probes are skipped. The run status becomes `Cancelled`.
///
/// # Errors
/// Returns [`ProbeError::Storage`] if a repository update fails.
pub async fn execute_probe_run(
    run_id: ProbeRunId,
    node_ids: Vec<NodeId>,
    probe_type: ProbeType,
    deps: RunnerDeps,
    cancelled: Arc<AtomicBool>,
    config: RunnerConfig,
) -> Result<(), ProbeError> {
    // If the run was cancelled before the runner picked it up (cancel_probe_run
    // marked it Cancelled in the DB directly because no flag was registered
    // yet), respect that and exit without overwriting the terminal status.
    if let Some(existing) = deps.run_repo.find_by_id(run_id).await?
        && existing.status.is_terminal()
    {
        return Ok(());
    }

    deps.run_repo
        .update_status(run_id, ProbeRunStatus::Running, &[], None)
        .await?;

    let semaphore = Arc::new(Semaphore::new(config.concurrency));
    let mut join_set: JoinSet<LatencyResult> = JoinSet::new();

    // Spawn a task per node, each acquiring a semaphore permit.
    for node_id in &node_ids {
        let node_id = *node_id;
        let permit_sem = Arc::clone(&semaphore);
        let probe = Arc::clone(&deps.probe);
        let pool = Arc::clone(&deps.pool_repo);
        let cancelled = Arc::clone(&cancelled);
        let timeout = config.timeout;

        join_set.spawn(async move {
            // If already cancelled before starting, skip.
            if cancelled.load(Ordering::Relaxed) {
                return LatencyResult {
                    node_id,
                    rtt_ms: None,
                    error_class: ErrorClass::Ok,
                };
            }
            // Acquire a permit (blocks if at concurrency limit).
            let _permit = permit_sem.acquire().await;
            // Re-check cancellation after acquiring.
            if cancelled.load(Ordering::Relaxed) {
                return LatencyResult {
                    node_id,
                    rtt_ms: None,
                    error_class: ErrorClass::Ok,
                };
            }
            // Fetch the node from the pool.
            let node = match pool.get_node(node_id).await {
                Ok(Some(entry)) => entry.node,
                Ok(None) => {
                    return LatencyResult {
                        node_id,
                        rtt_ms: None,
                        error_class: ErrorClass::DnsFailed,
                    };
                }
                Err(_) => {
                    return LatencyResult {
                        node_id,
                        rtt_ms: None,
                        error_class: ErrorClass::DnsFailed,
                    };
                }
            };
            // Probe with timeout.
            tokio::time::timeout(timeout, async { probe.probe(&node, timeout).await })
                .await
                .unwrap_or(LatencyResult {
                    node_id,
                    rtt_ms: None,
                    error_class: ErrorClass::Timeout,
                })
        });
    }

    // Collect results as tasks complete.
    let mut results: Vec<ProbeRunResult> = Vec::with_capacity(node_ids.len());
    let mut latency_records: Vec<LatencyRecord> = Vec::with_capacity(node_ids.len());
    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(r) => {
                let skipped = cancelled.load(Ordering::Relaxed)
                    && r.rtt_ms.is_none()
                    && r.error_class == ErrorClass::Ok;
                // Persist a latency record for every attempted (non-skipped)
                // probe so the latency query API has data. Skipped probes
                // (cancelled before start) produce no record.
                if !skipped {
                    latency_records.push(LatencyRecord {
                        id: LatencyRecordId::new(),
                        run_id,
                        node_id: r.node_id,
                        probe_type,
                        rtt_ms: r.rtt_ms,
                        error_class: r.error_class,
                        measured_at: Timestamp::now(),
                    });
                }
                results.push(ProbeRunResult {
                    node_id: r.node_id,
                    rtt_ms: r.rtt_ms,
                    error_class: r.error_class,
                    skipped,
                });
            }
            Err(_) => {
                // Task panicked; tokio::time::timeout already handles
                // timeouts. A JoinError here is a panic — the node's result
                // is simply absent from the results vector.
            }
        }
    }

    // Persist latency records. A storage failure here does not discard the
    // run results already collected — we log and continue so the probe run
    // status still reflects what the probes observed.
    for record in &latency_records {
        if let Err(e) = deps.latency_repo.create(record).await {
            tracing::warn!(
                error = %e,
                node_id = %record.node_id,
                run_id = %run_id,
                "failed to persist latency record"
            );
        }
    }

    // Determine final status.
    let is_cancelled = cancelled.load(Ordering::Relaxed);
    let (final_status, completed_at) = if is_cancelled {
        (ProbeRunStatus::Cancelled, Some(Timestamp::now()))
    } else {
        (ProbeRunStatus::Completed, Some(Timestamp::now()))
    };

    deps.run_repo
        .update_status(run_id, final_status, &results, completed_at)
        .await?;

    Ok(())
}
