//! Background scheduler that cleans up expired token-rotation grace rows.
//!
//! After a token rotation with a finite grace period, the old token digest is
//! retained in `previous_token_digest` so delivery can still serve it during
//! grace. Once `rotation_grace_until` passes, the old digest must be cleared
//! so the old token is permanently rejected. This scheduler periodically
//! sweeps expired grace rows.
//!
//! The scheduler is observable (traced per sweep), cancellable (shutdown
//! future breaks the loop), and safely shuts down — an in-progress sweep
//! completes before exit; no new sweep starts after shutdown (constraint #20).
//! See `docs/plan/milestones/M6-subscription-distribution.md` §"Token
//! rotation grace period" → Cleanup.

use std::time::Duration;

use deve_sub_domain::SubscriptionTokenRepository;

/// Default tick interval: sweep every 5 minutes.
const DEFAULT_TICK_SECS: u64 = 300;

/// Background scheduler that clears expired grace token rows.
pub struct GraceTokenCleanupScheduler {
    token_repo: std::sync::Arc<dyn SubscriptionTokenRepository>,
    tick_interval: Duration,
}

impl GraceTokenCleanupScheduler {
    /// Create a new scheduler with the given token repository and default
    /// tick interval.
    #[must_use]
    pub fn new(token_repo: std::sync::Arc<dyn SubscriptionTokenRepository>) -> Self {
        Self {
            token_repo,
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
    /// it calls `clear_expired_grace_tokens(now)` to NULL out
    /// `previous_token_digest` and `rotation_grace_until` for rows whose grace
    /// has expired.
    ///
    /// The shutdown signal is checked between ticks — an in-progress sweep is
    /// a single UPDATE and completes before the scheduler exits (safe
    /// shutdown per constraint #20).
    pub async fn run(self, shutdown: impl std::future::Future<Output = ()> + Send) {
        tokio::pin!(shutdown);
        tracing::info!(
            tick_secs = self.tick_interval.as_secs(),
            "grace token cleanup scheduler started"
        );
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::info!("grace token cleanup scheduler shutting down");
                    return;
                }
                _ = tokio::time::sleep(self.tick_interval) => {
                    self.tick().await;
                }
            }
        }
    }

    /// One scheduler tick: sweep expired grace tokens.
    async fn tick(&self) {
        let now = deve_sub_kernel::Timestamp::now();
        match self.token_repo.clear_expired_grace_tokens(now).await {
            Ok(0) => {
                tracing::debug!("grace cleanup: no expired tokens");
            }
            Ok(n) => {
                tracing::info!(cleaned = n, "grace cleanup: cleared expired tokens");
            }
            Err(e) => {
                tracing::warn!(error = %e, "grace cleanup: failed");
            }
        }
    }
}
