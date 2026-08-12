//! Background scheduler that aggregates daily traffic snapshots.
//!
//! Once per tick (default 24h), the scheduler computes the previous UTC day's
//! traffic totals per subscription and upserts a [`TrafficDailySnapshot`] row.
//! The scheduler is observable (traced per tick), cancellable (shutdown future
//! breaks the loop), and safely shuts down — an in-progress aggregation tick
//! completes before exit; no new tick starts after shutdown (constraint #20).
//! See `docs/plan/milestones/M10-observability-and-audit.md` §"Traffic daily
//! aggregation job".

use std::sync::Arc;
use std::time::Duration;

use deve_sub_domain::{TrafficDailySnapshotRepository, TrafficRepository};
use time::OffsetDateTime;

use crate::subscription::traffic_history::aggregate_daily_traffic;

/// Default tick interval: aggregate daily.
const DEFAULT_TICK_SECS: u64 = 86_400;

/// Background scheduler that aggregates daily traffic snapshots.
pub struct TrafficDailySnapshotScheduler {
    traffic_repo: Arc<dyn TrafficRepository>,
    snapshot_repo: Arc<dyn TrafficDailySnapshotRepository>,
    tick_interval: Duration,
}

impl TrafficDailySnapshotScheduler {
    /// Create a new scheduler with the given repositories and default tick
    /// interval (24h).
    #[must_use]
    pub fn new(
        traffic_repo: Arc<dyn TrafficRepository>,
        snapshot_repo: Arc<dyn TrafficDailySnapshotRepository>,
    ) -> Self {
        Self {
            traffic_repo,
            snapshot_repo,
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
    /// On each tick the scheduler aggregates the previous UTC day's traffic.
    /// The shutdown signal is checked between ticks — an in-progress
    /// aggregation completes before the scheduler exits (safe shutdown per
    /// constraint #20).
    pub async fn run(self, shutdown: impl std::future::Future<Output = ()> + Send) {
        tokio::pin!(shutdown);
        tracing::info!(
            tick_secs = self.tick_interval.as_secs(),
            "traffic daily snapshot scheduler started"
        );
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

    /// One scheduler tick: aggregate the previous UTC day's traffic into
    /// snapshots.
    async fn tick(&self) {
        match self.aggregate_yesterday().await {
            Ok(n) => {
                tracing::info!(snapshots = n, "traffic daily snapshot: aggregated");
            }
            Err(e) => {
                tracing::warn!(error = %e, "traffic daily snapshot: aggregation failed");
            }
        }
    }

    /// Aggregate traffic for the previous UTC day.
    ///
    /// Returns the number of subscription snapshots upserted.
    ///
    /// # Errors
    /// Propagates repository errors from the traffic or snapshot repositories.
    pub async fn aggregate_yesterday(&self) -> Result<usize, deve_sub_domain::SubscriptionError> {
        let now = OffsetDateTime::now_utc();
        let yesterday = now.date() - time::Duration::days(1);
        let day = format!(
            "{:04}-{:02}-{:02}",
            yesterday.year(),
            yesterday.month() as u8,
            yesterday.day()
        );
        let day_start = yesterday.midnight().assume_utc();
        let day_end = day_start + time::Duration::days(1);
        let day_start_iso = day_start
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| {
                deve_sub_domain::SubscriptionError::Storage(format!("timestamp format: {e}"))
            })?;
        let day_end_iso = day_end
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| {
                deve_sub_domain::SubscriptionError::Storage(format!("timestamp format: {e}"))
            })?;

        aggregate_daily_traffic(
            self.traffic_repo.as_ref(),
            self.snapshot_repo.as_ref(),
            &day,
            &day_start_iso,
            &day_end_iso,
        )
        .await
    }
}
