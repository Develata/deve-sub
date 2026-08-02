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
    #[allow(clippy::expect_used)]
    pub fn unix_ms(&self) -> i64 {
        // WHY: i64::try_from is infallible here because the representable
        // range of OffsetDateTime (±9999 years ≈ ±2.5×10¹⁴ ms) is far
        // within i64::MAX (9.22×10¹⁸) without the `large-dates` feature.
        i64::try_from(self.0.unix_timestamp_nanos() / 1_000_000)
            .expect("timestamp in milliseconds fits in i64 for any representable OffsetDateTime")
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
