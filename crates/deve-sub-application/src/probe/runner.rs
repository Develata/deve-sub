//! Probe runner: batch latency probing with bounded concurrency,
//! observable progress, and cancellation support (constraint #20).
//!
//! The runner is a built-in background job, not a separate service. It is
//! observable (ProbeRun status + per-node results queryable), cancellable
//! (cancellation flag, NODE-016), and safely shut down on server stop. See
//! `docs/plan/milestones/M7-probes-and-detection.md` §"Probe runner".

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use deve_sub_domain::{
    ErrorClass, LatencyProbe, LatencyRecord, LatencyRecordRepository, LatencyResult,
    NodePoolRepository, ProbeError, ProbeRunRepository, ProbeRunResult, ProbeRunStatus, ProbeType,
};
use deve_sub_kernel::{LatencyRecordId, NodeId, ProbeRunId, Timestamp};
use futures_util::stream::{self, StreamExt};

/// Default per-probe timeout (5 seconds).
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Default concurrency limit (32 concurrent probes).
pub const DEFAULT_CONCURRENCY: usize = 32;

/// Configuration for a probe runner execution.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub timeout: Duration,
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
    pub probe: Arc<dyn LatencyProbe>,
    pub pool_repo: Arc<dyn NodePoolRepository>,
    pub run_repo: Arc<dyn ProbeRunRepository>,
    pub latency_repo: Arc<dyn LatencyRecordRepository>,
}

/// Execute a probe run: probe each node with bounded concurrency, update
/// the run status and results as probes complete, and respect cancellation.
///
/// This is the public entry point. It wraps [`execute_probe_run_inner`] and
/// ensures that any unexpected error writes a `Failed` terminal status, so
/// no run is left in `Pending` or `Running` (B-14).
///
/// # Cancellation
/// If `cancelled` is set to `true`, in-flight probes are allowed to finish
/// their current attempt (bounded by `timeout`), and pending probes are
/// skipped. The run status becomes `Cancelled`.
///
/// # Errors
/// Returns [`ProbeError::Storage`] if a repository update fails. The `Failed`
/// status is written best-effort before returning.
pub async fn execute_probe_run(
    run_id: ProbeRunId,
    node_ids: Vec<NodeId>,
    probe_type: ProbeType,
    deps: RunnerDeps,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    config: RunnerConfig,
) -> Result<(), ProbeError> {
    let result = execute_probe_run_inner(
        run_id,
        node_ids,
        probe_type,
        deps.clone(),
        cancelled,
        config,
    )
    .await;

    if let Err(ref e) = result {
        tracing::error!(error = %e, %run_id, "probe run failed, writing Failed status");
        let _ = deps
            .run_repo
            .update_status(run_id, ProbeRunStatus::Failed, &[], Some(Timestamp::now()))
            .await;
    }

    result
}

async fn execute_probe_run_inner(
    run_id: ProbeRunId,
    node_ids: Vec<NodeId>,
    probe_type: ProbeType,
    deps: RunnerDeps,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    config: RunnerConfig,
) -> Result<(), ProbeError> {
    // If the run was cancelled before the runner picked it up, exit without
    // overwriting the terminal status.
    if let Some(existing) = deps.run_repo.find_by_id(run_id).await?
        && existing.status.is_terminal()
    {
        return Ok(());
    }

    // WHY: a cancel can arrive between the terminal check above and this
    // `Running` write. If so, the terminal guard returns `RunAlreadyTerminal`
    // and we continue — the cancel flag is already set, so all probes skip.
    // Non-terminal errors propagate so the outer wrapper writes `Failed`.
    if let Err(e) = deps
        .run_repo
        .update_status(run_id, ProbeRunStatus::Running, &[], None)
        .await
    {
        if matches!(e, ProbeError::RunAlreadyTerminal) {
            tracing::info!(%run_id, "probe run already terminal at Running transition; probes will skip");
        } else {
            return Err(e);
        }
    }

    // Dedup node IDs while preserving order (B-14).
    let mut seen = HashSet::new();
    let node_ids: Vec<NodeId> = node_ids.into_iter().filter(|id| seen.insert(*id)).collect();

    // Probe each node with bounded concurrency. `buffer_unordered` runs at
    // most `concurrency` futures at a time — unlike the previous JoinSet
    // approach which spawned a Tokio task per node (10k tasks for 10k nodes).
    let results: Vec<LatencyResult> = stream::iter(node_ids.iter().copied())
        .map(|node_id| {
            let probe = Arc::clone(&deps.probe);
            let pool = Arc::clone(&deps.pool_repo);
            let cancelled = Arc::clone(&cancelled);
            let timeout = config.timeout;
            async move {
                if cancelled.load(Ordering::Relaxed) {
                    return LatencyResult {
                        node_id,
                        rtt_ms: None,
                        error_class: ErrorClass::Ok,
                    };
                }
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
                // WHY: no outer tokio::time::timeout wrapper — each
                // LatencyProbe implementation applies its own internal
                // timeout to every I/O step using the same `timeout` budget
                // (W-Y). The probe is the single deadline authority.
                probe.probe(&node, timeout).await
            }
        })
        .buffer_unordered(config.concurrency)
        .collect()
        .await;

    // Process results into run results + latency records.
    let mut run_results: Vec<ProbeRunResult> = Vec::with_capacity(results.len());
    let mut latency_records: Vec<LatencyRecord> = Vec::with_capacity(results.len());
    for r in results {
        let skipped = cancelled.load(Ordering::Relaxed)
            && r.rtt_ms.is_none()
            && r.error_class == ErrorClass::Ok;
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
        run_results.push(ProbeRunResult {
            node_id: r.node_id,
            rtt_ms: r.rtt_ms,
            error_class: r.error_class,
            skipped,
        });
    }

    // Batch persist latency records (B-14). A storage failure here is logged,
    // not fatal — the run status still reflects what the probes observed.
    // `batch_create` is a no-op for empty slices.
    if let Err(e) = deps.latency_repo.batch_create(&latency_records).await {
        tracing::warn!(error = %e, %run_id, "failed to batch persist latency records");
    }

    // Determine final status.
    let is_cancelled = cancelled.load(Ordering::Relaxed);
    let (final_status, completed_at) = if is_cancelled {
        (ProbeRunStatus::Cancelled, Some(Timestamp::now()))
    } else {
        (ProbeRunStatus::Completed, Some(Timestamp::now()))
    };

    // WHY: a cancel can fire the flag AND persist `Cancelled` between our
    // `cancelled.load()` and this write. If so, the terminal guard returns
    // `RunAlreadyTerminal` — the user already received a 200 for `Cancelled`,
    // so we must not overwrite it. Still persist diagnostic results.
    match deps
        .run_repo
        .update_status(run_id, final_status, &run_results, completed_at)
        .await
    {
        Ok(()) => {}
        Err(ProbeError::RunAlreadyTerminal) => {
            tracing::info!(%run_id, "probe run became terminal via concurrent cancel; status write skipped");
            if let Err(e) = deps
                .run_repo
                .update_results(run_id, &run_results, completed_at)
                .await
            {
                tracing::warn!(error = %e, %run_id, "failed to persist diagnostic results after concurrent cancel");
            }
            return Ok(());
        }
        Err(e) => return Err(e),
    }

    Ok(())
}
