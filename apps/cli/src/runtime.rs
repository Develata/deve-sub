//! Composition-root lifecycle for operational sampling and task shutdown.

use std::sync::Arc;
use std::time::Duration;

use deve_sub_domain::AuditLogRepository;

use deve_sub_application::{JobSupervisor, LoginRateLimiter};
use deve_sub_storage_sqlite::SqliteMaintenance;

/// Periodic task reaping, bounded-cardinality telemetry and WAL maintenance.
/// SQL details remain entirely in the SQLite adapter.
pub async fn maintain(
    maintenance: Arc<SqliteMaintenance>,
    audit: Arc<dyn AuditLogRepository>,
    audit_retention_days: u32,
    jobs: Arc<JobSupervisor>,
    limiter: Arc<dyn LoginRateLimiter>,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = shutdown.recv() => return,
            _ = interval.tick() => {
                let reaped = jobs.reap_finished();
                tracing::info!(
                    reaped,
                    tracked_jobs = jobs.len(),
                    task_panics = jobs.panic_count(),
                    task_cancellations = jobs.cancellation_count(),
                    rate_limiter_entries = limiter.resident_entries(),
                    "runtime resources"
                );
                let pruning = tokio::time::timeout(Duration::from_secs(10), async {
                    for _ in 0..10 {
                        if prune_round(&maintenance, audit.as_ref(), audit_retention_days).await == 0 { break; }
                    }

                });
                let result = tokio::select! {
                    biased;
                    _ = shutdown.recv() => return,
                    result = pruning => result,
                };
                match result {
                    Ok(()) => {}
                    Err(_) => tracing::warn!("history retention timed out"),
                }
                checkpoint(&maintenance).await;
            }
        }
    }
}

/// Bound pool acquisition as well as the checkpoint query. The WAL remains
/// valid after an interrupted PASSIVE checkpoint; the next tick retries.
pub async fn checkpoint(maintenance: &SqliteMaintenance) {
    match tokio::time::timeout(Duration::from_secs(10), maintenance.checkpoint()).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => tracing::warn!(%error, "sqlite checkpoint failed"),
        Err(_) => tracing::warn!("sqlite checkpoint timed out"),
    }
}

// Each history family gets an attempt even if the other is corrupt. Otherwise
// an audit receipt failure would indefinitely disable session/outbox retention.
async fn prune_round(
    maintenance: &SqliteMaintenance,
    audit: &dyn AuditLogRepository,
    days: u32,
) -> u64 {
    let history = match maintenance.prune_history().await {
        Ok(deleted) => deleted,
        Err(error) => {
            tracing::warn!(%error, "sqlite retention failed");
            0
        }
    };
    let audit = match deve_sub_application::audit::prune_audit_logs(audit, days).await {
        Ok(deleted) => deleted,
        Err(error) => {
            tracing::warn!(%error, "audit retention failed");
            0
        }
    };
    history + audit
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
