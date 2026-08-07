//! Background auto-refresh scheduler for sources with `auto_update = true`.
//!
//! Periodically scans enabled auto-update sources and refreshes those whose
//! `update_interval_secs` has elapsed since the last snapshot. The scheduler
//! is observable (traced per refresh), cancellable (shutdown future breaks
//! the loop), and safely shuts down (in-progress refreshes complete before
//! exit; no new refreshes start after shutdown — constraint #20).

use std::time::Duration;

use deve_sub_domain::{NodePoolRepository, SourceRepository, SourceSnapshotRepository};

use super::commands::refresh_source;
use super::fetcher::SubscriptionFetcher;
use super::geoip::GeoIpPort;

/// Default tick interval: check for due sources every 60 seconds.
const DEFAULT_TICK_SECS: u64 = 60;

/// Background scheduler that auto-refreshes eligible sources.
pub struct RefreshScheduler {
    source_repo: std::sync::Arc<dyn SourceRepository>,
    snapshot_repo: std::sync::Arc<dyn SourceSnapshotRepository>,
    pool_repo: std::sync::Arc<dyn NodePoolRepository>,
    fetcher: std::sync::Arc<dyn SubscriptionFetcher>,
    geoip: std::sync::Arc<dyn GeoIpPort>,
    tick_interval: Duration,
}

impl RefreshScheduler {
    /// Create a new scheduler with the given dependencies and default tick.
    #[must_use]
    pub fn new(
        source_repo: std::sync::Arc<dyn SourceRepository>,
        snapshot_repo: std::sync::Arc<dyn SourceSnapshotRepository>,
        pool_repo: std::sync::Arc<dyn NodePoolRepository>,
        fetcher: std::sync::Arc<dyn SubscriptionFetcher>,
        geoip: std::sync::Arc<dyn GeoIpPort>,
    ) -> Self {
        Self {
            source_repo,
            snapshot_repo,
            pool_repo,
            fetcher,
            geoip,
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
    /// Between ticks, the scheduler sleeps for `tick_interval`. On each tick,
    /// it pages through all sources, filters for `auto_update && enabled`,
    /// checks whether the last snapshot is due, and refreshes if so.
    ///
    /// The shutdown signal is checked between ticks only — an in-progress
    /// refresh is allowed to complete before the scheduler exits (safe
    /// shutdown per constraint #20).
    pub async fn run(self, shutdown: impl std::future::Future<Output = ()> + Send) {
        tokio::pin!(shutdown);
        tracing::info!(
            tick_secs = self.tick_interval.as_secs(),
            "refresh scheduler started"
        );
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::info!("refresh scheduler shutting down");
                    return;
                }
                _ = tokio::time::sleep(self.tick_interval) => {
                    self.tick().await;
                }
            }
        }
    }

    /// One scheduler tick: scan all sources, refresh those that are due
    /// concurrently (SRC-013). Each refresh is independent — separate
    /// source_id, separate snapshot, separate reconcile transaction — so
    /// concurrent execution cannot cross-pollute.
    async fn tick(&self) {
        let due = self.collect_due_sources().await;
        if due.is_empty() {
            return;
        }
        tracing::info!(count = due.len(), "refreshing due sources concurrently");

        let mut set = tokio::task::JoinSet::new();
        for source_id in due {
            let source_repo = self.source_repo.clone();
            let snapshot_repo = self.snapshot_repo.clone();
            let pool_repo = self.pool_repo.clone();
            let fetcher = self.fetcher.clone();
            let geoip = self.geoip.clone();
            set.spawn(async move {
                let result = refresh_source(
                    source_repo.as_ref(),
                    snapshot_repo.as_ref(),
                    pool_repo.as_ref(),
                    fetcher.as_ref(),
                    geoip.as_ref(),
                    source_id,
                )
                .await;
                (source_id, result)
            });
        }

        while let Some(join_result) = set.join_next().await {
            match join_result {
                Ok((source_id, result)) => match &result {
                    Ok(r) => {
                        if r.not_modified {
                            tracing::info!(source = %source_id, "auto-refresh: not modified");
                        } else {
                            tracing::info!(
                                source = %source_id,
                                version = r.snapshot.version,
                                new = r.reconcile.new_nodes,
                                dup = r.reconcile.duplicate_nodes,
                                reactivated = r.reconcile.reactivated_nodes,
                                missing = r.reconcile.missing_nodes,
                                "auto-refresh: completed"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(source = %source_id, error = %e, "auto-refresh: failed");
                    }
                },
                Err(join_err) => {
                    tracing::warn!(error = %join_err, "refresh task panicked");
                }
            }
        }
    }

    /// Page through all sources and return IDs of those that are due for
    /// refresh (auto_update && enabled && interval elapsed since last
    /// snapshot, or no snapshot yet).
    async fn collect_due_sources(&self) -> Vec<deve_sub_kernel::SourceId> {
        let mut due = Vec::new();
        let mut cursor: Option<deve_sub_kernel::SourceId> = None;
        let now = deve_sub_kernel::Timestamp::now();
        let page_size: u32 = 100;

        loop {
            let page = match self.source_repo.list(cursor, page_size).await {
                Ok(sources) => sources,
                Err(e) => {
                    tracing::warn!(error = %e, "scheduler: failed to list sources");
                    return due;
                }
            };
            if page.is_empty() {
                break;
            }
            let next_cursor = page.last().map(|s| s.id);
            for source in &page {
                if !source.auto_update || !source.enabled {
                    continue;
                }
                match self.snapshot_repo.find_active(source.id).await {
                    Ok(Some(snapshot)) => {
                        let elapsed =
                            now.as_offset_date_time() - snapshot.fetched_at.as_offset_date_time();
                        if elapsed.whole_seconds().max(0) as u64 >= source.update_interval_secs {
                            due.push(source.id);
                        }
                    }
                    Ok(None) => due.push(source.id),
                    Err(e) => {
                        tracing::warn!(
                            source = %source.id,
                            error = %e,
                            "scheduler: failed to find active snapshot"
                        );
                    }
                }
            }
            if page.len() < page_size as usize {
                break;
            }
            cursor = next_cursor;
        }
        due
    }
}
