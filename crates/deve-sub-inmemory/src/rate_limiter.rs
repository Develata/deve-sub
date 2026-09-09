//! Process-local login failure tracking with a hard resident-key bound.
//!
//! Full capacity fails closed for unknown keys instead of evicting active
//! lockouts. Pressure cleanup only removes expired entries, at most once per
//! second. Fixed-size, domain-separated digests bound memory even for long
//! attacker-controlled usernames. State is intentionally lost on restart.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use deve_sub_application::auth::{AuthError, LoginRateLimiter};
use sha2::{Digest, Sha256};

/// Maximum combined number of username and IP failure records.
const MAX_ENTRIES: usize = 10_000;

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

    fn keys(username: &str, ip: Option<&str>) -> impl Iterator<Item = [u8; 32]> {
        [Some(Self::key(0, username)), ip.map(|ip| Self::key(1, ip))]
            .into_iter()
            .flatten()
    }

    fn sweep_at_capacity(&self, state: &mut State, now: Instant) {
        if state.entries.len() < MAX_ENTRIES || now < state.next_sweep {
            return;
        }
        // WHY: preserve the existing pressure-eviction semantics, including
        // sticky failure counts below capacity. Never evict an active lockout;
        // otherwise rotating usernames would buy fresh brute-force budgets.
        let max_age = self.lockout_duration.saturating_mul(2);
        state.entries.retain(|_, entry| match entry.locked_until {
            Some(until) => until > now,
            None => now.duration_since(entry.last_failure) < max_age,
        });
        state.next_sweep = now + Duration::from_secs(1);
    }
}

impl LoginRateLimiter for InMemoryLoginRateLimiter {
    fn check(&self, username: &str, ip: Option<&str>) -> Result<(), AuthError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        self.sweep_at_capacity(&mut state, now);
        let mut missing = 0;
        for key in Self::keys(username, ip) {
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
        if missing > MAX_ENTRIES - state.entries.len() {
            return Err(AuthError::RateLimited);
        }
        Ok(())
    }

    fn record_failure(&self, username: &str, ip: Option<&str>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        self.sweep_at_capacity(&mut state, now);
        for key in Self::keys(username, ip) {
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
