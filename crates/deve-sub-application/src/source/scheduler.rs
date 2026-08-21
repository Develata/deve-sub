//! Background auto-refresh scheduler for sources with `auto_update = true`.
//!
//! Periodically scans enabled auto-update sources and refreshes those whose
//! `update_interval_secs` has elapsed since the last snapshot. The scheduler
//! is observable (traced per refresh), cancellable (shutdown future breaks
//! the loop), and safely shuts down (in-progress refreshes complete before
//! exit; no new refreshes start after shutdown — constraint #20).
//!
//! B-15: each refresh now goes through the job lifecycle
//! (`start_refresh_job` → `execute_refresh_job`) so progress is tracked in
//! the `source_refresh_jobs` table and the per-source lease prevents
//! concurrent refreshes.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use deve_sub_domain::{
    NodePoolRepository, SourceRefreshJobRepository, SourceRepository, SourceSnapshotRepository,
};

use super::fetcher::SubscriptionFetcher;
use super::geoip::GeoIpPort;
use super::refresh::{RefreshDeps, execute_refresh_job, start_refresh_job};

const DEFAULT_TICK_SECS: u64 = 60;
const DEFAULT_MAX_CONCURRENCY: usize = 4;
/// A refresh job whose `started_at` is older than this is presumed dead
/// (killed, panicked without unwind, or host lost power) and its lease is
/// reclaimed at each tick (P0-10).
const DEFAULT_LEASE_TIMEOUT_SECS: u64 = 600;

pub struct RefreshScheduler {
    source_repo: std::sync::Arc<dyn SourceRepository>,
    snapshot_repo: std::sync::Arc<dyn SourceSnapshotRepository>,
    pool_repo: std::sync::Arc<dyn NodePoolRepository>,
    job_repo: std::sync::Arc<dyn SourceRefreshJobRepository>,
    fetcher: std::sync::Arc<dyn SubscriptionFetcher>,
    geoip: std::sync::Arc<dyn GeoIpPort>,
    tick_interval: Duration,
    max_concurrency: usize,
    lease_timeout: Duration,
}

impl RefreshScheduler {
    #[must_use]
    pub fn new(
        source_repo: std::sync::Arc<dyn SourceRepository>,
        snapshot_repo: std::sync::Arc<dyn SourceSnapshotRepository>,
        pool_repo: std::sync::Arc<dyn NodePoolRepository>,
        job_repo: std::sync::Arc<dyn SourceRefreshJobRepository>,
        fetcher: std::sync::Arc<dyn SubscriptionFetcher>,
        geoip: std::sync::Arc<dyn GeoIpPort>,
    ) -> Self {
        Self {
            source_repo,
            snapshot_repo,
            pool_repo,
            job_repo,
            fetcher,
            geoip,
            tick_interval: Duration::from_secs(DEFAULT_TICK_SECS),
            max_concurrency: DEFAULT_MAX_CONCURRENCY,
            lease_timeout: Duration::from_secs(DEFAULT_LEASE_TIMEOUT_SECS),
        }
    }

    #[must_use]
    pub fn tick_interval(mut self, interval: Duration) -> Self {
        self.tick_interval = interval;
        self
    }

    #[must_use]
    pub fn max_concurrency(mut self, max: usize) -> Self {
        self.max_concurrency = max.max(1);
        self
    }

    /// Set the lease timeout after which a `Running` refresh job is presumed
    /// dead and its lease is reclaimed at each scheduler tick (P0-10).
    #[must_use]
    pub fn lease_timeout(mut self, timeout: Duration) -> Self {
        self.lease_timeout = timeout;
        self
    }

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
    /// concurrent execution cannot cross-pollute. Concurrency is capped by
    /// `max_concurrency` via a semaphore to bound resource usage.
    ///
    /// B-15: each refresh goes through the job lifecycle. The per-source
    /// lease (DB partial unique index on `source_refresh_jobs`) prevents
    /// concurrent refreshes even if the scheduler and a manual API call
    /// race.
    ///
    /// P0-10: before scanning due sources, reclaim leases held by `Running`
    /// jobs older than `lease_timeout`. A job stuck in `Running` (runner
    /// killed, panicked without unwind, or host lost power) otherwise
    /// blocks all future refreshes for that source for the entire uptime.
    async fn tick(&self) {
        self.reclaim_stale_leases().await;
        let due = self.collect_due_sources().await;
        if due.is_empty() {
            return;
        }
        tracing::info!(
            count = due.len(),
            max_concurrency = self.max_concurrency,
            "refreshing due sources concurrently"
        );

        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(self.max_concurrency));
        let mut set = tokio::task::JoinSet::new();
        for source_id in due {
            let source_repo = self.source_repo.clone();
            let snapshot_repo = self.snapshot_repo.clone();
            let pool_repo = self.pool_repo.clone();
            let job_repo = self.job_repo.clone();
            let fetcher = self.fetcher.clone();
            let geoip = self.geoip.clone();
            let permit = semaphore.clone();
            set.spawn(async move {
                #[allow(clippy::expect_used, reason = "semaphore is never closed by us")]
                let _permit = permit
                    .acquire_owned()
                    .await
                    .expect("scheduler semaphore is never closed");
                let deps = RefreshDeps {
                    source_repo: source_repo.as_ref(),
                    snapshot_repo: snapshot_repo.as_ref(),
                    pool_repo: pool_repo.as_ref(),
                    job_repo: job_repo.as_ref(),
                    fetcher: fetcher.as_ref(),
                    geoip: geoip.as_ref(),
                };
                let cancelled = AtomicBool::new(false);
                match start_refresh_job(&deps, source_id).await {
                    Ok(job_id) => {
                        let result =
                            execute_refresh_job(&deps, job_id, source_id, &cancelled).await;
                        (source_id, result)
                    }
                    Err(e) => (source_id, Err(e)),
                }
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

    async fn collect_due_sources(&self) -> Vec<deve_sub_kernel::SourceId> {
        let now = deve_sub_kernel::Timestamp::now();
        let page_size: u32 = 100;

        let mut candidates: Vec<deve_sub_domain::source::Source> = Vec::new();
        let mut cursor: Option<deve_sub_kernel::SourceId> = None;
        loop {
            let page = match self.source_repo.list(cursor, page_size).await {
                Ok(sources) => sources,
                Err(e) => {
                    tracing::warn!(error = %e, "scheduler: failed to list sources");
                    return Vec::new();
                }
            };
            if page.is_empty() {
                break;
            }
            let next_cursor = page.last().map(|s| s.id);
            for source in page {
                if source.auto_update && source.enabled {
                    candidates.push(source);
                }
            }
            if next_cursor.is_none() {
                break;
            }
            cursor = next_cursor;
        }

        if candidates.is_empty() {
            return Vec::new();
        }

        let candidate_ids: Vec<deve_sub_kernel::SourceId> =
            candidates.iter().map(|s| s.id).collect();
        // WHY: batch-fetch all active snapshots in one query instead of N
        // find_active calls (W-EE). Sources absent from the map have no
        // snapshot yet and are due immediately.
        let snapshots = match self
            .snapshot_repo
            .find_active_for_sources(&candidate_ids)
            .await
        {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "scheduler: failed to batch-fetch active snapshots");
                return Vec::new();
            }
        };

        let mut due = Vec::new();
        for source in &candidates {
            match snapshots.get(&source.id) {
                Some(snapshot) => {
                    let elapsed =
                        now.as_offset_date_time() - snapshot.fetched_at.as_offset_date_time();
                    if elapsed.whole_seconds().max(0) as u64 >= source.update_interval_secs {
                        due.push(source.id);
                    }
                }
                None => due.push(source.id),
            }
        }
        due
    }

    /// Reclaim leases held by `Running` refresh jobs older than
    /// `lease_timeout` (P0-10).
    ///
    /// A job stuck in `Running` — the runner task was killed, panicked
    /// without unwinding, or the host lost power — holds the per-source
    /// lease via the partial UNIQUE index `idx_refresh_jobs_lease` and
    /// blocks all future refreshes for that source. This sweep marks
    /// those jobs as `Failed` so the lease is released and the next tick
    /// can start a fresh refresh.
    async fn reclaim_stale_leases(&self) {
        let now = deve_sub_kernel::Timestamp::now();
        let cutoff = now
            - time::Duration::seconds(
                i64::try_from(self.lease_timeout.as_secs()).unwrap_or(i64::MAX),
            );
        match super::refresh::recover_stale_refresh_jobs(
            self.job_repo.as_ref(),
            cutoff,
            "lease timed out (scheduler stale-lease sweep)",
        )
        .await
        {
            Ok(n) if n > 0 => {
                tracing::info!(recovered = n, "reclaimed stale refresh job leases");
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(error = %e, "failed to reclaim stale refresh job leases");
            }
        }
    }
}
