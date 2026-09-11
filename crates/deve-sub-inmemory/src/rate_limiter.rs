//! Process-local login failure tracking with a hard resident-key bound.
//!
//! Pressure evicts old probation records, never active lockouts or identities
//! within the current limiter call. Fixed-size, domain-separated digests bound
//! memory even for long usernames. State is intentionally lost on restart.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use deve_sub_application::auth::{AuthError, LoginRateLimiter};
use sha2::{Digest, Sha256};

/// Maximum combined number of username and IP failure records.
const MAX_ENTRIES: usize = 10_000;
const EVICTION_BATCH: usize = 256;

/// Bounded in-memory implementation of [`LoginRateLimiter`].
pub struct InMemoryLoginRateLimiter {
    max_attempts: u32,
    lockout_duration: Duration,
    state: Mutex<State>,
}

struct State {
    entries: HashMap<[u8; 32], RateLimitEntry>,
    next_sweep: Instant,
}

struct RateLimitEntry {
    failed_attempts: u32,
    locked_until: Option<Instant>,
    last_failure: Instant,
}

impl InMemoryLoginRateLimiter {
    /// Create a limiter with a shared 10,000-entry username/IP budget.
    #[must_use]
    pub fn new(max_attempts: u32, lockout_duration: Duration) -> Self {
        Self {
            max_attempts,
            lockout_duration,
            state: Mutex::new(State {
                entries: HashMap::new(),
                next_sweep: Instant::now(),
            }),
        }
    }

    fn key(namespace: u8, value: &str) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update([namespace]);
        hash.update(value.as_bytes());
        hash.finalize().into()
    }

    fn keys(username: &str, ip: Option<&str>) -> [Option<[u8; 32]>; 2] {
        [Some(Self::key(0, username)), ip.map(|ip| Self::key(1, ip))]
    }

    fn make_room(&self, state: &mut State, keys: &[Option<[u8; 32]>; 2], now: Instant) {
        let missing = keys
            .iter()
            .flatten()
            .filter(|key| !state.entries.contains_key(*key))
            .count();
        if missing <= MAX_ENTRIES - state.entries.len() || now < state.next_sweep {
            return;
        }
        // Preserve this attempt's existing IP/username counters: username
        // rotation must not evict an IP immediately before it reaches lockout.
        let before = state.entries.len();
        let max_age = self.lockout_duration.saturating_mul(2);
        state.entries.retain(|key, entry| {
            keys.contains(&Some(*key))
                || match entry.locked_until {
                    Some(until) => until > now,
                    None => now.duration_since(entry.last_failure) < max_age,
                }
        });
        if missing <= MAX_ENTRIES - state.entries.len() {
            if before - state.entries.len() < EVICTION_BATCH {
                state.next_sweep = now + Duration::from_secs(1);
            }
            return;
        }
        // At most 10k candidates; selection is linear and a batch amortizes
        // the scan over 256 freed key slots. A full batch needs no time-based
        // denial window; low-yield scans are throttled below.
        let mut candidates: Vec<_> = state
            .entries
            .iter()
            .filter(|(key, entry)| {
                !keys.contains(&Some(**key)) && entry.locked_until.is_none_or(|until| until <= now)
            })
            .map(|(key, entry)| (entry.last_failure, *key))
            .collect();
        let evicted = candidates.len().min(EVICTION_BATCH);
        if evicted < candidates.len() {
            candidates.select_nth_unstable(evicted);
        }
        for (_, key) in candidates.into_iter().take(evicted) {
            state.entries.remove(&key);
        }
        if before - state.entries.len() < EVICTION_BATCH {
            // An almost entirely locked table must not scan 10k entries for
            // every single probation eviction. Existing free slots remain
            // usable; reclamation is reconsidered within one second.
            state.next_sweep = now + Duration::from_secs(1);
            tracing::warn!(
                entries = state.entries.len(),
                "login capacity dominated by lockouts"
            );
        }
        tracing::debug!(
            evicted,
            entries = state.entries.len(),
            "login capacity reclaimed"
        );
    }
}

impl LoginRateLimiter for InMemoryLoginRateLimiter {
    fn check(&self, username: &str, ip: Option<&str>) -> Result<(), AuthError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let keys = Self::keys(username, ip);
        let mut missing = 0;
        for key in keys.into_iter().flatten() {
            match state.entries.get_mut(&key) {
                Some(entry) => {
                    if let Some(until) = entry.locked_until {
                        if until > now {
                            return Err(AuthError::RateLimited);
                        }
                        // Retain the count: one failure after expiry re-locks.
                        entry.locked_until = None;
                    }
                }
                None => missing += 1,
            }
        }
        self.make_room(&mut state, &keys, now);
        if missing > MAX_ENTRIES - state.entries.len() {
            return Err(AuthError::RateLimited);
        }
        Ok(())
    }

    fn record_failure(&self, username: &str, ip: Option<&str>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let keys = Self::keys(username, ip);
        self.make_room(&mut state, &keys, now);
        for key in keys.into_iter().flatten() {
            // Concurrent checks do not reserve slots. Recheck under the same
            // mutex as insertion so racing failures cannot exceed the bound.
            if !state.entries.contains_key(&key) && state.entries.len() == MAX_ENTRIES {
                continue;
            }
            let entry = state.entries.entry(key).or_insert(RateLimitEntry {
                failed_attempts: 0,
                locked_until: None,
                last_failure: now,
            });
            entry.failed_attempts = entry.failed_attempts.saturating_add(1);
            entry.last_failure = now;
            if entry.failed_attempts >= self.max_attempts {
                entry.locked_until = now.checked_add(self.lockout_duration);
            }
        }
    }

    fn record_success(&self, username: &str, _ip: Option<&str>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // A successful login must never clear the shared IP's failure count.
        state.entries.remove(&Self::key(0, username));
    }

    fn resident_entries(&self) -> Option<usize> {
        Some(
            self.state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entries
                .len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probation_pressure_allows_new_identity_without_flushing_active_lockouts() {
        let limiter = InMemoryLoginRateLimiter::new(5, Duration::from_secs(3600));
        for _ in 0..5 {
            limiter.record_failure("victim", Some("192.0.2.1"));
        }
        for i in 0..100_000 {
            let username = format!("rotating-{i}");
            let ip = format!("synthetic-ip-{i}");
            assert!(limiter.check(&username, Some(&ip)).is_ok(), "admission {i}");
            limiter.record_failure(&username, Some(&ip));
            assert!(limiter.resident_entries().expect("count") <= MAX_ENTRIES);
        }
        assert!(limiter.check("clean-user", Some("192.0.2.2")).is_ok());
        assert!(limiter.check("victim", None).is_err());
        assert!(limiter.check("other-user", Some("192.0.2.1")).is_err());
    }

    #[test]
    fn two_new_keys_reclaim_expired_entries_with_one_slot_remaining() {
        let limiter = InMemoryLoginRateLimiter::new(5, Duration::from_secs(60));
        for i in 0..MAX_ENTRIES - 1 {
            limiter.record_failure(&format!("expired-{i}"), None);
        }
        {
            let mut state = limiter.state.lock().expect("lock");
            let expired = Instant::now() - Duration::from_secs(121);
            for entry in state.entries.values_mut() {
                entry.last_failure = expired;
            }
        }
        assert_eq!(limiter.resident_entries(), Some(MAX_ENTRIES - 1));
        assert!(limiter.check("new", Some("192.0.2.3")).is_ok());
        assert_eq!(limiter.resident_entries(), Some(0));
    }

    #[test]
    fn pressure_preserves_current_ip_probation_until_it_locks() {
        let limiter = InMemoryLoginRateLimiter::new(5, Duration::from_secs(3600));
        limiter.record_failure("initial", Some("192.0.2.4"));
        for i in 0..MAX_ENTRIES - 2 {
            limiter.record_failure(&format!("filler-{i}"), None);
        }
        for i in 0..4 {
            let user = format!("same-ip-{i}");
            assert!(limiter.check(&user, Some("192.0.2.4")).is_ok());
            limiter.record_failure(&user, Some("192.0.2.4"));
        }
        assert!(limiter.check("new", Some("192.0.2.4")).is_err());
        limiter.record_success("same-ip-3", Some("192.0.2.4"));
        assert!(limiter.check("new", Some("192.0.2.4")).is_err());
        assert!(limiter.check("same-ip-3", Some("192.0.2.5")).is_ok());
    }

    #[test]
    fn nearly_all_locked_pressure_does_not_rescan_for_each_probation() {
        let limiter = InMemoryLoginRateLimiter::new(2, Duration::from_secs(3600));
        for i in 0..MAX_ENTRIES - 1 {
            let user = format!("locked-{i}");
            limiter.record_failure(&user, None);
            limiter.record_failure(&user, None);
        }
        limiter.record_failure("probation", None);
        assert!(limiter.check("first", None).is_ok());
        let deadline = limiter.state.lock().expect("lock").next_sweep;
        assert!(deadline > Instant::now());
        limiter.record_failure("first", None);
        assert_eq!(limiter.state.lock().expect("lock").next_sweep, deadline);
        assert!(limiter.check("second", None).is_err());
        assert!(limiter.check("locked-0", None).is_err());
        // Advance the maintenance state rather than asserting wall time.
        limiter.state.lock().expect("lock").next_sweep = Instant::now();
        assert!(limiter.check("second", None).is_ok());
    }

    #[test]
    fn high_cardinality_is_hard_bounded_and_does_not_evict_lockouts() {
        let limiter = InMemoryLoginRateLimiter::new(1, Duration::from_secs(3600));
        limiter.record_failure("protected", None);
        for i in 0..100_000 {
            limiter.record_failure(&format!("user-{i}"), Some(&format!("ip-{i}")));
            assert!(limiter.resident_entries().expect("local entries") <= MAX_ENTRIES);
        }
        assert_eq!(limiter.resident_entries(), Some(MAX_ENTRIES));
        assert!(matches!(
            limiter.check("protected", None),
            Err(AuthError::RateLimited)
        ));
        assert!(matches!(
            limiter.check("new", None),
            Err(AuthError::RateLimited)
        ));
    }

    #[test]
    fn expired_pressure_entries_allow_admission_again() {
        let limiter = InMemoryLoginRateLimiter::new(1, Duration::from_secs(60));
        for i in 0..MAX_ENTRIES {
            limiter.record_failure(&format!("user-{i}"), None);
        }
        {
            let mut state = limiter.state.lock().expect("lock");
            let expired = Instant::now() - Duration::from_secs(1);
            for entry in state.entries.values_mut() {
                entry.locked_until = Some(expired);
            }
            state.next_sweep = expired;
        }
        assert!(limiter.check("new", None).is_ok());
        assert_eq!(limiter.resident_entries(), Some(0));
        limiter.record_failure("new", None);
        assert!(matches!(
            limiter.check("new", None),
            Err(AuthError::RateLimited)
        ));
    }

    #[test]
    fn username_cannot_alias_or_reset_ip_key() {
        let limiter = InMemoryLoginRateLimiter::new(1, Duration::from_secs(60));
        limiter.record_failure("alice", Some("192.0.2.1"));
        limiter.record_success("ip:192.0.2.1", None);
        assert!(matches!(
            limiter.check("bob", Some("192.0.2.1")),
            Err(AuthError::RateLimited)
        ));
        assert!(limiter.check("ip:192.0.2.1", None).is_ok());
    }

    #[test]
    fn allows_login_under_threshold() {
        let limiter = InMemoryLoginRateLimiter::new(3, Duration::from_secs(60));
        assert!(limiter.check("alice", None).is_ok());
        limiter.record_failure("alice", None);
        assert!(limiter.check("alice", None).is_ok());
        limiter.record_failure("alice", None);
        assert!(limiter.check("alice", None).is_ok());
    }

    #[test]
    fn locks_after_threshold() {
        let limiter = InMemoryLoginRateLimiter::new(3, Duration::from_secs(60));
        for _ in 0..3 {
            limiter.record_failure("alice", None);
        }
        assert!(matches!(
            limiter.check("alice", None),
            Err(AuthError::RateLimited)
        ));
    }

    #[test]
    fn success_resets_counter() {
        let limiter = InMemoryLoginRateLimiter::new(3, Duration::from_secs(60));
        limiter.record_failure("alice", None);
        limiter.record_failure("alice", None);
        limiter.record_success("alice", None);
        // Should be back to 0 failures — 1 more failure should not lock.
        limiter.record_failure("alice", None);
        assert!(limiter.check("alice", None).is_ok());
    }

    #[test]
    fn ip_and_username_tracked_independently() {
        let limiter = InMemoryLoginRateLimiter::new(2, Duration::from_secs(60));
        // Fail twice as alice from 10.0.0.1 — both username and IP locked.
        limiter.record_failure("alice", Some("10.0.0.1"));
        limiter.record_failure("alice", Some("10.0.0.1"));
        // alice is locked.
        assert!(matches!(
            limiter.check("alice", Some("10.0.0.2")),
            Err(AuthError::RateLimited)
        ));
        // IP 10.0.0.1 is also locked — even bob from that IP is blocked.
        assert!(matches!(
            limiter.check("bob", Some("10.0.0.1")),
            Err(AuthError::RateLimited)
        ));
        // bob from a different IP is fine.
        assert!(limiter.check("bob", Some("10.0.0.2")).is_ok());
    }

    #[test]
    fn lockout_expires() {
        let limiter = InMemoryLoginRateLimiter::new(1, Duration::from_millis(10));
        limiter.record_failure("alice", None);
        assert!(matches!(
            limiter.check("alice", None),
            Err(AuthError::RateLimited)
        ));
        // Wait for lockout to expire.
        std::thread::sleep(Duration::from_millis(20));
        // Should be allowed now — lockout expired (counter retained, not
        // reset — see P0-12 hard-cap test below).
        assert!(limiter.check("alice", None).is_ok());
    }

    /// P0-12: after lockout expiry, the failure count is retained so a
    /// single new failure immediately re-locks. This reduces the brute-force
    /// rate from max_attempts per lockout cycle to 1 per cycle.
    #[test]
    fn p0_12_hard_cap_re_locks_after_single_failure() {
        let limiter = InMemoryLoginRateLimiter::new(3, Duration::from_millis(10));
        // Exhaust the initial budget: 3 failures → locked.
        limiter.record_failure("alice", None);
        limiter.record_failure("alice", None);
        limiter.record_failure("alice", None);
        assert!(matches!(
            limiter.check("alice", None),
            Err(AuthError::RateLimited)
        ));
        // Wait for lockout to expire.
        std::thread::sleep(Duration::from_millis(20));
        // Lockout expired — check passes (1 try granted).
        assert!(limiter.check("alice", None).is_ok());
        // A single failure immediately re-locks because failed_attempts
        // is still 3 (>= max_attempts).
        limiter.record_failure("alice", None);
        assert!(matches!(
            limiter.check("alice", None),
            Err(AuthError::RateLimited)
        ));
    }

    /// P0-12: record_success clears the counter, so a legitimate user who
    /// fat-fingered their password and waited out the lockout gets a full
    /// fresh budget after a successful login.
    #[test]
    fn p0_12_success_clears_counter_after_lockout() {
        let limiter = InMemoryLoginRateLimiter::new(2, Duration::from_millis(10));
        limiter.record_failure("alice", None);
        limiter.record_failure("alice", None);
        assert!(matches!(
            limiter.check("alice", None),
            Err(AuthError::RateLimited)
        ));
        std::thread::sleep(Duration::from_millis(20));
        // Lockout expired — check passes.
        assert!(limiter.check("alice", None).is_ok());
        // Successful login clears the counter.
        limiter.record_success("alice", None);
        // Should now have a full fresh budget (2 tries).
        limiter.record_failure("alice", None);
        assert!(limiter.check("alice", None).is_ok());
    }
}
