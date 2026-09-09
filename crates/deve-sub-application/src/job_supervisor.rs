//! Bounded background-task ownership and shutdown (constraint #20).

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::task::{JoinError, JoinSet};

/// Maximum concurrently tracked request-triggered jobs. Completed jobs are
/// reaped before admission; callers reject excess work instead of queueing it.
const MAX_JOBS: usize = 64;

/// Why a new job could not be admitted.
#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    /// The process is draining and must not accept additional work.
    #[error("background jobs are shutting down")]
    ShuttingDown,
    /// The fixed concurrent-job budget is exhausted.
    #[error("background job capacity reached")]
    AtCapacity,
}

struct State {
    tasks: JoinSet<()>,
    closed: bool,
}

/// Owns request-triggered jobs, reaping completions during normal operation.
///
/// `spawn` and `reap_finished` never await while holding the mutex. Closing
/// admission and taking ownership for shutdown happen under that same mutex,
/// so a racing spawn cannot escape the shutdown drain.
pub struct JobSupervisor {
    state: Mutex<State>,
    panics: AtomicU64,
    cancellations: AtomicU64,
}

impl JobSupervisor {
    /// Create an empty supervisor with a fixed concurrency budget.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                tasks: JoinSet::new(),
                closed: false,
            }),
            panics: AtomicU64::new(0),
            cancellations: AtomicU64::new(0),
        }
    }

    /// Admit a job or return a capacity/shutdown error, dropping its future.
    /// The future must handle its business errors and persist terminal status.
    pub fn spawn<F>(&self, job: F) -> Result<(), SpawnError>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.drain_finished(&mut state.tasks);
        if state.closed {
            return Err(SpawnError::ShuttingDown);
        }
        if state.tasks.len() >= MAX_JOBS {
            return Err(SpawnError::AtCapacity);
        }
        state.tasks.spawn(job);
        Ok(())
    }

    fn record(&self, result: Result<(), JoinError>) {
        if let Err(error) = result {
            if error.is_panic() {
                self.panics.fetch_add(1, Ordering::Relaxed);
                // Panic payloads may contain credentials; only log task identity.
                tracing::error!(task_id = %error.id(), "background task panicked");
            } else {
                self.cancellations.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(task_id = %error.id(), "background task cancelled");
            }
        }
    }

    fn drain_finished(&self, tasks: &mut JoinSet<()>) -> usize {
        let mut count = 0;
        while let Some(result) = tasks.try_join_next() {
            self.record(result);
            count += 1;
        }
        count
    }

    /// Reclaim completed jobs without waiting for running jobs.
    pub fn reap_finished(&self) -> usize {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.drain_finished(&mut state.tasks)
    }

    /// Close admission immediately; existing jobs remain owned until shutdown.
    pub fn close(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
    }

    /// Close admission and drain, aborting jobs after `timeout`.
    ///
    /// Cancellation destructors normally run immediately. Their drain has a
    /// separate one-second bound because Tokio cannot forcibly stop a future
    /// that blocks a runtime thread without yielding. Dropping the remaining
    /// JoinSet requests cancellation again; process exit is the final boundary.
    pub async fn shutdown(&self, timeout: Duration) {
        let mut tasks = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
            std::mem::take(&mut state.tasks)
        };
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match tokio::time::timeout_at(deadline, tasks.join_next()).await {
                Ok(Some(result)) => self.record(result),
                Ok(None) => return,
                Err(_) => break,
            }
        }
        tracing::warn!(
            remaining = tasks.len(),
            "background job grace expired; aborting"
        );
        tasks.abort_all();
        if tokio::time::timeout(Duration::from_secs(1), async {
            while let Some(result) = tasks.join_next().await {
                self.record(result);
            }
        })
        .await
        .is_err()
        {
            tracing::error!(
                remaining = tasks.len(),
                "background jobs did not yield after abort"
            );
        }
    }

    /// Number of running or completed-but-not-yet-reaped tasks (at most 64).
    #[must_use]
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .tasks
            .len()
    }

    /// Whether there are any tracked tasks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Cumulative task panics, without per-job metric labels.
    #[must_use]
    pub fn panic_count(&self) -> u64 {
        self.panics.load(Ordering::Relaxed)
    }

    /// Cumulative joined cancellations, including forced shutdown aborts.
    #[must_use]
    pub fn cancellation_count(&self) -> u64 {
        self.cancellations.load(Ordering::Relaxed)
    }
}

impl Default for JobSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "job_supervisor_tests.rs"]
mod tests;
