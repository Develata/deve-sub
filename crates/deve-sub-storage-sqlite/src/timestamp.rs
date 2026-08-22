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
    // WHY: truncate to whole seconds and force UTC so every stored timestamp
    // is the canonical "YYYY-MM-DDTHH:MM:SSZ" shape, matching the strftime
    // defaults in migrations. Subsecond digits break string-compared range
    // queries: ".5Z" sorts lexicographically BEFORE "Z", so a record written
    // at T00:00:00.5 would land in the previous day's range (e.g.
    // summaries_by_subscription_in_range); a non-UTC offset breaks string
    // comparison entirely.
    let truncated = ts
        .as_offset_date_time()
        .to_offset(time::UtcOffset::UTC)
        .replace_nanosecond(0)
        .map_err(|e| format!("timestamp truncate error: {e}"))?;
    truncated
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

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    #[test]
    fn format_ts_truncates_subsecond_and_forces_utc() {
        let dt = OffsetDateTime::parse("2026-08-23T00:00:00.5Z", &Rfc3339).expect("parse");
        let s = format_ts(Timestamp::from_offset_date_time(dt)).expect("format");
        assert_eq!(s, "2026-08-23T00:00:00Z");
    }

    #[test]
    fn format_ts_converts_non_utc_offset_to_utc() {
        let dt = OffsetDateTime::parse("2026-08-23T08:00:00.25+08:00", &Rfc3339).expect("parse");
        let s = format_ts(Timestamp::from_offset_date_time(dt)).expect("format");
        assert_eq!(s, "2026-08-23T00:00:00Z");
    }

    #[test]
    fn format_ts_output_sorts_correctly_against_day_boundary() {
        let midnight_next = "2026-08-24T00:00:00Z";
        let record = format_ts(Timestamp::from_offset_date_time(
            OffsetDateTime::parse("2026-08-24T00:00:00.999Z", &Rfc3339).expect("parse"),
        ))
        .expect("format");
        // A record at the day boundary must NOT sort before the exclusive
        // end boundary (previous-day mis-bucketing).
        assert!(record.as_str() >= midnight_next);
    }
}
