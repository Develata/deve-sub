//! Bounded history cleanup. Cutoffs and table names are adapter-owned constants.

use sqlx::SqlitePool;

use crate::StorageError;

/// Delete one bounded batch per history, preserving live work and lifetime totals.
///
/// Each statement commits independently. Cancellation may stop between tables;
/// replay is idempotent and never requires a full-history transaction or VACUUM.
pub(crate) async fn prune(pool: &SqlitePool) -> Result<u64, StorageError> {
    let policies = [
        (
            "subscription_traffic",
            "recorded_at < strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-30 days')",
        ),
        ("traffic_daily_snapshots", "date < date('now', '-400 days')"),
        (
            "probe_runs",
            "status IN ('C', 'X', 'F') AND completed_at < strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-30 days')",
        ),
        (
            "source_refresh_jobs",
            "status IN ('C', 'F', 'X') AND finished_at < strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-30 days')",
        ),
        (
            "sessions",
            "expires_at < strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        ),
        (
            "subscription_temp_links",
            "expires_at < strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        ),
        (
            "outbox_event",
            "processed_at IS NOT NULL AND processed_at < strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-30 days')",
        ),
    ];
    let mut total = 0;
    for (table, predicate) in policies {
        // WHY: LIMIT in the selection works on SQLite builds without the
        // optional DELETE LIMIT extension. Constants cannot carry user SQL.
        let result = sqlx::query(&format!(
            "DELETE FROM {table} WHERE rowid IN (SELECT rowid FROM {table} WHERE {predicate} LIMIT 500)"
        ))
        .execute(pool).await?;
        total += result.rows_affected();
        tracing::info!(
            table,
            pruned = result.rows_affected(),
            batch_limit = 500,
            "sqlite retention"
        );
    }
    Ok(total)
}
