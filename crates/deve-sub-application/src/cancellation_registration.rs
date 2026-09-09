//! Cancellation registry ownership for request-triggered background jobs.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Shared cancellation flags; only live jobs retain registrations.
pub type CancellationFlags<K> = Arc<Mutex<HashMap<K, Arc<AtomicBool>>>>;

/// Removes a job's cancellation flag on completion, panic, abort, or rejected
/// admission. Construct before spawning and move the guard into the future:
/// this also covers a future dropped before its first poll.
pub struct CancellationRegistration<K: Eq + Hash> {
    flags: CancellationFlags<K>,
    id: K,
}

impl<K: Eq + Hash + Clone> CancellationRegistration<K> {
    /// Register a fresh job identifier. Identifiers must be unique per job.
    pub fn new(flags: CancellationFlags<K>, id: K, cancelled: Arc<AtomicBool>) -> Self {
        flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), cancelled);
        Self { flags, id }
    }
}

impl<K: Eq + Hash> Drop for CancellationRegistration<K> {
    fn drop(&mut self) {
        self.flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JobSupervisor;
    use std::time::Duration;

    #[tokio::test]
    async fn registrations_are_removed_after_abort_panic_and_rejection() {
        let flags = CancellationFlags::default();
        let supervisor = JobSupervisor::new();
        for id in 0..2 {
            let guard =
                CancellationRegistration::new(flags.clone(), id, Arc::new(AtomicBool::new(false)));
            supervisor
                .spawn(async move {
                    let _guard = guard;
                    if id == 0 {
                        panic!("test panic");
                    }
                    std::future::pending::<()>().await;
                })
                .expect("spawn");
        }
        tokio::task::yield_now().await;
        supervisor.shutdown(Duration::from_millis(1)).await;
        assert!(flags.lock().expect("flags").is_empty());
        let guard =
            CancellationRegistration::new(flags.clone(), 2, Arc::new(AtomicBool::new(false)));
        assert!(
            supervisor
                .spawn(async move {
                    let _guard = guard;
                })
                .is_err()
        );
        assert!(flags.lock().expect("flags").is_empty());
    }
}
