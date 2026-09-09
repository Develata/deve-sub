//! SQLite snapshot and verification queries used by backup and restore.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::COUNTED_TABLES;

pub(super) async fn current_schema_version(pool: &sqlx::sqlite::SqlitePool) -> Result<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .context("failed to query schema version")?;
    Ok(row.0)
}

pub(super) async fn collect_row_counts(
    pool: &sqlx::sqlite::SqlitePool,
) -> Result<BTreeMap<String, i64>> {
    let mut counts = BTreeMap::new();
    for table in COUNTED_TABLES {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        match sqlx::query_as::<_, (i64,)>(sql.as_str())
            .fetch_one(pool)
            .await
        {
            Ok((count,)) => {
                counts.insert((*table).to_owned(), count);
            }
            Err(e) => {
                tracing::debug!(table, error = %e, "skipping row count for missing/inaccessible table");
            }
        }
    }
    Ok(counts)
}

pub(super) async fn vacuum_into(pool: &sqlx::sqlite::SqlitePool, target: &Path) -> Result<()> {
    let target_str = target
        .to_str()
        .context("snapshot path is not valid UTF-8")?;
    // Defense-in-depth: VACUUM INTO uses single-quote string interpolation.
    // The target is currently an internal tempfile path, but reject quotes
    // to prevent SQL injection if the path ever becomes user-controlled.
    if target_str.contains('\'') {
        bail!("snapshot path contains a single quote — refusing to interpolate into VACUUM INTO");
    }
    let sql = format!("VACUUM INTO '{target_str}'");
    sqlx::query(&sql)
        .execute(pool)
        .await
        .context("VACUUM INTO failed")?;
    Ok(())
}

pub(super) async fn integrity_check(pool: &sqlx::sqlite::SqlitePool) -> Result<String> {
    let row: (String,) = sqlx::query_as("PRAGMA integrity_check")
        .fetch_one(pool)
        .await
        .context("integrity_check query failed")?;
    Ok(row.0)
}
