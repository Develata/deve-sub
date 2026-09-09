//! Composition-root lifecycle for operational sampling and task shutdown.

use std::sync::Arc;
use std::time::Duration;

use deve_sub_application::{JobSupervisor, LoginRateLimiter};
use deve_sub_storage_sqlite::SqliteMaintenance;

/// Periodic task reaping, bounded-cardinality telemetry and WAL maintenance.
/// SQL details remain entirely in the SQLite adapter.
pub async fn maintain(
    maintenance: Arc<SqliteMaintenance>,
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
                match tokio::time::timeout(Duration::from_secs(10), async {
                    for _ in 0..10 {
                        if maintenance.prune_history().await? == 0 { break; }
                    }
                    Ok::<(), deve_sub_storage_sqlite::StorageError>(())
                }).await {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => tracing::warn!(%error, "sqlite retention failed"),
                    Err(_) => tracing::warn!("sqlite retention timed out"),
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
