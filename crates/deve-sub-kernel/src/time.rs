//! UTC timestamp primitive used across the workspace.

use serde::{Deserialize, Serialize};

use crate::error::{KernelError, Result};

/// A point in time stored as UTC.
///
/// Wraps [`time::OffsetDateTime`] to provide a stable workspace-internal type
/// that does not leak the `time` crate into public APIs of downstream crates
/// unless they choose to re-export it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Timestamp(time::OffsetDateTime);

impl Timestamp {
    /// Returns the current UTC time.
    #[must_use]
    pub fn now() -> Self {
        Self(time::OffsetDateTime::now_utc())
    }

    /// Create from a Unix timestamp in milliseconds.
    ///
    /// # Errors
    /// Returns [`KernelError::InvalidTimestamp`] if the value is out of range.
    pub fn from_unix_ms(ms: i64) -> Result<Self> {
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
            .map(Self)
            .map_err(|_| KernelError::InvalidTimestamp)
    }

    /// Return the Unix timestamp in milliseconds.
    #[must_use]
    pub fn unix_ms(&self) -> i64 {
        i64::try_from(self.0.unix_timestamp_nanos() / 1_000_000)
            .expect("timestamp in milliseconds fits in i64 for any representable OffsetDateTime")
    }

    /// Return the inner [`time::OffsetDateTime`].
    #[must_use]
    pub const fn as_inner(&self) -> time::OffsetDateTime {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_and_unix_ms_roundtrip() {
        let ts = Timestamp::now();
        let ms = ts.unix_ms();
        let recovered = Timestamp::from_unix_ms(ms).expect("roundtrip should succeed");
        assert_eq!(ts.unix_ms(), recovered.unix_ms());
    }

    #[test]
    fn from_unix_ms_invalid() {
        assert!(Timestamp::from_unix_ms(i64::MAX).is_err());
    }

    #[test]
    fn ordering() {
        let a = Timestamp::now();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = Timestamp::now();
        assert!(a < b);
    }
}
