//! Unified background job supervisor (constraint #20).
//!
//! Tracks spawned background tasks in a [`JoinSet`] and provides a
//! timeout-bounded graceful shutdown. On shutdown, the caller should first
//! signal jobs to cancel (e.g. set cancellation flags), then call
//! [`JobSupervisor::shutdown`]. Tasks that do not finish within the timeout
//! are aborted; their associated state (e.g. probe run rows left in
//! `Running`) is recovered on the next process start via
//! `recover_crashed_runs`.

use std::sync::Mutex;
use std::time::Duration;

use tokio::task::JoinSet;

/// Supervises background tasks, ensuring they are tracked, joinable, and
/// safely shut down.
///
/// Stored in [`AppState`](crate::AppState) as `Arc<JobSupervisor>` and
/// shared across all route handlers. The `Mutex<JoinSet>` is cheap —
/// `spawn` and `shutdown` are the only operations, and contention is low
/// (probe runs are infrequent relative to HTTP traffic).
pub struct JobSupervisor {
    tasks: Mutex<JoinSet<()>>,
}

impl JobSupervisor {
    /// Create a new empty supervisor.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(JoinSet::new()),
        }
    }

    /// Spawn a background task under this supervisor's tracking.
    ///
    /// The task's `Output` must be `()` — callers are expected to handle
    /// errors inside the closure (e.g. write a `Failed` terminal status)
    /// so that the supervisor does not need domain-specific knowledge.
    pub fn spawn<F>(&self, job: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .spawn(job);
    }

    /// Gracefully shut down all tracked tasks within `timeout`.
    ///
    /// Waits for each task to complete. Tasks that do not finish before
    /// the deadline are aborted. Aborted tasks may leave domain state in
    /// a non-terminal state (e.g. probe runs stuck in `Running`); the
    /// caller is responsible for crash recovery on the next start.
    pub async fn shutdown(&self, timeout: Duration) {
        // Take the JoinSet out of the mutex so the lock is not held across
        // await points (clippy::await_holding_lock). New `spawn` calls during
        // shutdown add to a fresh empty JoinSet inside the mutex; those tasks
        // are intentionally not drained — shutdown means stop accepting work.
        let mut tasks = {
            let mut guard = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut *guard)
        };

        if tasks.is_empty() {
            return;
        }

        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match tokio::time::timeout_at(deadline, tasks.join_next()).await {
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(_) => {
                    let remaining = tasks.len();
                    tasks.abort_all();
                    tracing::warn!(
                        remaining,
                        "job supervisor shutdown timeout, aborted remaining tasks"
                    );
                    break;
                }
            }
        }
    }

    /// Returns the number of currently tracked tasks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Returns `true` if no tasks are tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for JobSupervisor {
    fn default() -> Self {
        Self::new()
    }
}
