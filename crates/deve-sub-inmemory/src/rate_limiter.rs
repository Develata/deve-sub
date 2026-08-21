//! In-memory login rate limiter.
//!
//! Tracks failed login attempts per username and per IP address using a
//! `Mutex<HashMap>`. After `max_attempts` failures, the key is locked for
//! `lockout_duration`. This is process-local state — it is not shared across
//! instances and is lost on restart. For a self-hosted single-binary product,
//! this is sufficient and avoids a database migration (AUTH-004).
//!
//! The rate limiter is intentionally in-memory rather than database-backed.
//! Rationale: per-IP tracking cannot use columns on the `users` table, and
//! in-memory rate limiting is simpler, faster, and adequate for a
//! single-instance deployment. The lockout resets on restart, which is
//! acceptable for a self-hosted product.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use deve_sub_application::auth::{AuthError, LoginRateLimiter};

/// In-memory implementation of [`LoginRateLimiter`].
pub struct InMemoryLoginRateLimiter {
    max_attempts: u32,
    lockout_duration: Duration,
    entries: Mutex<HashMap<String, RateLimitEntry>>,
}

struct RateLimitEntry {
    failed_attempts: u32,
    locked_until: Option<Instant>,
    /// When the last failure was recorded. Used by `evict_expired` to
    /// remove stale entries and prevent unbounded HashMap growth.
    last_failure: Instant,
}

/// Maximum number of entries before triggering eviction of expired entries.
/// WHY: prevents unbounded HashMap growth from attacker-generated unique
/// keys (e.g., rotating XFF values). 10_000 entries × ~100 bytes ≈ 1 MB.
const MAX_ENTRIES: usize = 10_000;

impl InMemoryLoginRateLimiter {
    /// Create a new rate limiter with the given threshold and lockout
    /// duration.
    #[must_use]
    pub fn new(max_attempts: u32, lockout_duration: Duration) -> Self {
        Self {
            max_attempts,
            lockout_duration,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Keys to check/record for a given username and optional IP.
    fn keys(username: &str, ip: Option<&str>) -> Vec<String> {
        let mut keys = vec![username.to_owned()];
        if let Some(ip) = ip {
            keys.push(format!("ip:{ip}"));
        }
        keys
    }

    /// Evict entries whose lockout has expired OR whose last failure is
    /// older than `2 × lockout_duration`. Called when the map exceeds
    /// `MAX_ENTRIES` to prevent unbounded memory growth.
    ///
    /// WHY: eviction is a memory-boundary mechanism, not a security decay
    /// policy. When the map is under `MAX_ENTRIES`, stale failure counts may
    /// persist longer than `2 × lockout_duration` — the security policy is
    /// enforced by `check` (lockout expiry resets the counter) and
    /// `record_success` (clears the username key), not by eviction. The
    /// inconsistency is acceptable: a few hundred stale entries cost
    /// negligible memory, and their counts only matter if the same key
    /// accumulates further failures, at which point the existing count
    /// contributes to a lockout — the desired behavior.
    fn evict_expired(entries: &mut HashMap<String, RateLimitEntry>, lockout_duration: Duration) {
        let now = Instant::now();
        let max_age = lockout_duration * 2;
        entries.retain(|_, entry| {
            if let Some(locked_until) = entry.locked_until {
                // Keep entries that are still locked.
                locked_until > now
            } else {
                // Keep entries with recent failures (within 2× lockout
                // duration). Older entries are stale and can be evicted.
                now.duration_since(entry.last_failure) < max_age
            }
        });
    }
}

impl LoginRateLimiter for InMemoryLoginRateLimiter {
    fn check(&self, username: &str, ip: Option<&str>) -> Result<(), AuthError> {
        // WHY: recover from a poisoned mutex rather than panicking. A poisoned
        // mutex means another thread panicked while holding the lock; the data
        // may be stale but is still usable for a rate limiter.
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();

        for key in Self::keys(username, ip) {
            if let Some(entry) = entries.get_mut(&key)
                && let Some(locked_until) = entry.locked_until
            {
                if locked_until > now {
                    return Err(AuthError::RateLimited);
                }
                // WHY (P0-12): lockout expired — clear the lockout but KEEP
                // the failure count. Previously the counter was reset to 0,
                // letting an attacker try max_attempts passwords every
                // lockout_duration cycle. Keeping the count means the next
                // single failure immediately re-locks (failed_attempts is
                // already >= max_attempts), reducing the brute-force rate
                // from max_attempts per cycle to 1 per cycle. Only a
                // successful login (record_success) clears the counter.
                entry.locked_until = None;
            }
        }

        Ok(())
    }

    fn record_failure(&self, username: &str, ip: Option<&str>) {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();

        // WHY: evict expired entries when the map grows large to prevent
        // unbounded memory growth from attacker-generated unique keys.
        if entries.len() > MAX_ENTRIES {
            Self::evict_expired(&mut entries, self.lockout_duration);
        }

        for key in Self::keys(username, ip) {
            let entry = entries.entry(key).or_insert(RateLimitEntry {
                failed_attempts: 0,
                locked_until: None,
                last_failure: now,
            });
            entry.failed_attempts = entry.failed_attempts.saturating_add(1);
            entry.last_failure = now;
            if entry.failed_attempts >= self.max_attempts {
                entry.locked_until = Some(now + self.lockout_duration);
            }
        }
    }

    fn record_success(&self, username: &str, _ip: Option<&str>) {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        // WHY: only remove the username key, not the IP key. Removing the
        // IP key would let an attacker on a shared IP reset the IP-level
        // counter by successfully logging in as themselves, then resume
        // attacking other users from the same IP.
        entries.remove(username);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
