//! Shared timestamp conversion helpers for SQLite repositories.
//!
//! Timestamps are stored as RFC 3339 strings, matching the `strftime`
//! default in migration 0002. The helpers return `Result<_, String>` so any
//! domain error enum can adapt them via `map_err(Storage)`. See ADR-0002.

use deve_sub_kernel::Timestamp;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Format a [`Timestamp`] as an RFC 3339 string for SQLite storage.
///
/// # Errors
/// Returns a human-readable string if formatting fails.
pub fn format_ts(ts: Timestamp) -> Result<String, String> {
    ts.as_offset_date_time()
        .format(&Rfc3339)
        .map_err(|e| format!("timestamp format error: {e}"))
}

/// Parse an RFC 3339 string from SQLite into a [`Timestamp`].
///
/// # Errors
/// Returns a human-readable string if parsing fails.
pub fn parse_ts(s: &str) -> Result<Timestamp, String> {
    OffsetDateTime::parse(s, &Rfc3339)
        .map(Timestamp::from_offset_date_time)
        .map_err(|e| format!("timestamp parse error: {e}"))
}
