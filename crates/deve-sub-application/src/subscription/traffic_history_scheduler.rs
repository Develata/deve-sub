//! Background scheduler for daily traffic aggregation and retention.
//!
//! Once per tick (default 24h), the scheduler aggregates the previous UTC
//! days' traffic totals per subscription into [`TrafficDailySnapshot`] rows
//! and prunes expired probe runs. On startup (and after each restart) it
//! re-aggregates the last `BACKFILL_DAYS` days so downtime does not leave
//! permanent holes in traffic history — aggregation is an idempotent upsert
//! per day, so re-running a completed day is safe.
//!
//! The scheduler is observable (traced per tick), cancellable (shutdown
//! future breaks the loop), and safely shuts down — an in-progress tick
//! completes before exit; no new tick starts after shutdown (constraint
//! #20). See `docs/plan/milestones/M10-observability-and-audit.md`
//! §"Traffic daily aggregation job".

use std::sync::Arc;
use std::time::Duration;

use deve_sub_domain::{ProbeRunRepository, TrafficDailySnapshotRepository, TrafficRepository};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::subscription::traffic_history::aggregate_daily_traffic;

/// Default tick interval: aggregate daily.
const DEFAULT_TICK_SECS: u64 = 86_400;

/// How many past UTC days each tick re-aggregates. Covers downtime gaps:
/// aggregation is an idempotent per-day upsert, so days already aggregated
/// are simply overwritten with the same totals.
const BACKFILL_DAYS: i64 = 7;

/// Probe runs older than this many days are pruned on each tick (cascades
/// their latency records).
const PROBE_RUN_RETENTION_DAYS: i64 = 30;

/// Background scheduler that aggregates daily traffic snapshots and prunes
/// expired probe data.
pub struct TrafficDailySnapshotScheduler {
    traffic_repo: Arc<dyn TrafficRepository>,
    snapshot_repo: Arc<dyn TrafficDailySnapshotRepository>,
    probe_run_repo: Arc<dyn ProbeRunRepository>,
    tick_interval: Duration,
}

impl TrafficDailySnapshotScheduler {
    /// Create a new scheduler with the given repositories and default tick
    /// interval (24h).
    #[must_use]
    pub fn new(
        traffic_repo: Arc<dyn TrafficRepository>,
        snapshot_repo: Arc<dyn TrafficDailySnapshotRepository>,
        probe_run_repo: Arc<dyn ProbeRunRepository>,
    ) -> Self {
        Self {
            traffic_repo,
            snapshot_repo,
            probe_run_repo,
            tick_interval: Duration::from_secs(DEFAULT_TICK_SECS),
        }
    }

    /// Set the tick interval.
    #[must_use]
    pub fn tick_interval(mut self, interval: Duration) -> Self {
        self.tick_interval = interval;
        self
    }

    /// Run the scheduler loop until `shutdown` completes.
    ///
    /// One tick runs immediately at startup (so restarts do not skip a day),
    /// then once per tick interval. The shutdown signal is checked between
    /// ticks — an in-progress tick completes before the scheduler exits
    /// (safe shutdown per constraint #20).
    pub async fn run(self, shutdown: impl std::future::Future<Output = ()> + Send) {
        tokio::pin!(shutdown);
        tracing::info!(
            tick_secs = self.tick_interval.as_secs(),
            "traffic daily snapshot scheduler started"
        );
        self.tick().await;
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::info!("traffic daily snapshot scheduler shutting down");
                    return;
                }
                _ = tokio::time::sleep(self.tick_interval) => {
                    self.tick().await;
                }
            }
        }
    }

    /// One scheduler tick: re-aggregate the recent traffic window and prune
    /// expired probe runs.
    async fn tick(&self) {
        match self.aggregate_recent_days().await {
            Ok(n) => {
                tracing::info!(snapshots = n, "traffic daily snapshot: aggregated");
            }
            Err(e) => {
                tracing::warn!(error = %e, "traffic daily snapshot: aggregation failed");
            }
        }
        self.prune_probe_runs().await;
    }

    /// Aggregate traffic for the last `BACKFILL_DAYS` UTC days (yesterday
    /// inclusive). Days already aggregated are upserted with identical
    /// totals, so this is idempotent.
    ///
    /// Returns the number of subscription snapshots upserted.
    ///
    /// # Errors
    /// Propagates repository errors from the traffic or snapshot
    /// repositories. A failure aborts the remaining days; the next tick
    /// retries the window from scratch.
    pub async fn aggregate_recent_days(&self) -> Result<usize, deve_sub_domain::SubscriptionError> {
        let now = OffsetDateTime::now_utc();
        let mut total = 0;
        for days_ago in 1..=BACKFILL_DAYS {
            let day_date = now.date() - time::Duration::days(days_ago);
            let day_start = day_date.midnight().assume_utc();
            let day_end = day_start + time::Duration::days(1);
            let day_start_iso = format_iso(day_start)?;
            let day_end_iso = format_iso(day_end)?;
            total += aggregate_daily_traffic(
                self.traffic_repo.as_ref(),
                self.snapshot_repo.as_ref(),
                &format!(
                    "{:04}-{:02}-{:02}",
                    day_date.year(),
                    day_date.month() as u8,
                    day_date.day()
                ),
                &day_start_iso,
                &day_end_iso,
            )
            .await?;
        }
        Ok(total)
    }

    /// Prune probe runs (and cascaded latency records) older than the
    /// retention window. Failures are logged, not propagated — retention is
    /// best-effort and retried on the next tick.
    async fn prune_probe_runs(&self) {
        let cutoff = OffsetDateTime::now_utc() - time::Duration::days(PROBE_RUN_RETENTION_DAYS);
        match self
            .probe_run_repo
            .prune_older_than(deve_sub_kernel::Timestamp::from_offset_date_time(cutoff))
            .await
        {
            Ok(0) => {}
            Ok(n) => {
                tracing::info!(runs = n, "traffic daily snapshot: pruned old probe runs");
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "traffic daily snapshot: probe run prune failed"
                );
            }
        }
    }
}

/// Format an instant as whole-second UTC RFC 3339, matching the canonical
/// storage shape (`format_ts`) so string comparison in the prune query is
/// exact.
fn format_iso(t: OffsetDateTime) -> Result<String, deve_sub_domain::SubscriptionError> {
    t.replace_nanosecond(0)
        .map_err(|e| deve_sub_domain::SubscriptionError::Storage(format!("timestamp: {e}")))?
        .format(&Rfc3339)
        .map_err(|e| deve_sub_domain::SubscriptionError::Storage(format!("timestamp format: {e}")))
}
