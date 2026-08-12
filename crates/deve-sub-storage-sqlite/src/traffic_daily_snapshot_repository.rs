//! SQLite implementation of [`TrafficDailySnapshotRepository`].
//!
//! Daily snapshots are upserted by the M10 aggregation job. The
//! `(subscription_id, date)` UNIQUE constraint makes upsert idempotent.
//! See `docs/plan/milestones/M10-observability-and-audit.md`.

use std::collections::BTreeMap;

use async_trait::async_trait;
use deve_sub_domain::{
    SubscriptionError, TrafficDailySnapshot, TrafficDailySnapshotRepository, TrafficSourceKind,
};
use deve_sub_kernel::SubscriptionId;
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

pub struct SqliteTrafficDailySnapshotRepository {
    pool: SqlitePool,
}

impl SqliteTrafficDailySnapshotRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct SnapshotRow {
    subscription_id: String,
    date: String,
    total_upload: i64,
    total_download: i64,
    source_breakdown_json: String,
    computed_at: String,
}

fn parse_breakdown(
    json_str: &str,
) -> Result<Vec<(TrafficSourceKind, u64, u64)>, SubscriptionError> {
    if json_str.is_empty() || json_str == "{}" {
        return Ok(Vec::new());
    }
    let map: BTreeMap<String, (i64, i64)> =
        serde_json::from_str(json_str).map_err(|e| SubscriptionError::Storage(e.to_string()))?;
    let mut out = Vec::with_capacity(map.len());
    for (key, (up, down)) in map {
        let kind = TrafficSourceKind::from_db_char(&key).ok_or_else(|| {
            SubscriptionError::Storage(format!("unknown source_kind '{key}' in breakdown"))
        })?;
        out.push((kind, up.max(0) as u64, down.max(0) as u64));
    }
    Ok(out)
}

fn row_to_domain(row: SnapshotRow) -> Result<TrafficDailySnapshot, SubscriptionError> {
    let subscription_id = SubscriptionId::parse(&row.subscription_id)
        .map_err(|e| SubscriptionError::Storage(format!("invalid subscription id: {e}")))?;
    let source_breakdown = parse_breakdown(&row.source_breakdown_json)?;
    let computed_at = parse_ts(&row.computed_at).map_err(SubscriptionError::Storage)?;
    Ok(TrafficDailySnapshot {
        subscription_id,
        date: row.date,
        total_upload: row.total_upload.max(0) as u64,
        total_download: row.total_download.max(0) as u64,
        source_breakdown,
        computed_at,
    })
}

#[async_trait]
impl TrafficDailySnapshotRepository for SqliteTrafficDailySnapshotRepository {
    async fn upsert(&self, snapshot: &TrafficDailySnapshot) -> Result<(), SubscriptionError> {
        let computed_at = format_ts(snapshot.computed_at).map_err(SubscriptionError::Storage)?;

        let mut breakdown_map: BTreeMap<&str, (i64, i64)> = BTreeMap::new();
        for (kind, up, down) in &snapshot.source_breakdown {
            breakdown_map.insert(kind.as_db_char(), (*up as i64, *down as i64));
        }
        let breakdown_json = serde_json::to_string(&breakdown_map)
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;

        sqlx::query(
            "INSERT INTO traffic_daily_snapshots \
             (subscription_id, date, total_upload, total_download, source_breakdown_json, computed_at) \
             VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(subscription_id, date) DO UPDATE SET \
             total_upload = excluded.total_upload, \
             total_download = excluded.total_download, \
             source_breakdown_json = excluded.source_breakdown_json, \
             computed_at = excluded.computed_at",
        )
        .bind(snapshot.subscription_id.to_string())
        .bind(&snapshot.date)
        .bind(snapshot.total_upload as i64)
        .bind(snapshot.total_download as i64)
        .bind(&breakdown_json)
        .bind(&computed_at)
        .execute(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn list_for_subscription(
        &self,
        subscription_id: SubscriptionId,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<TrafficDailySnapshot>, SubscriptionError> {
        let rows: Vec<SnapshotRow> = sqlx::query_as(
            "SELECT subscription_id, date, total_upload, total_download, source_breakdown_json, computed_at \
             FROM traffic_daily_snapshots \
             WHERE subscription_id = ? AND date >= ? AND date <= ? \
             ORDER BY date ASC",
        )
        .bind(subscription_id.to_string())
        .bind(start_date)
        .bind(end_date)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        rows.into_iter().map(row_to_domain).collect()
    }

    async fn list_global(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<TrafficDailySnapshot>, SubscriptionError> {
        let rows: Vec<SnapshotRow> = sqlx::query_as(
            "SELECT subscription_id, date, total_upload, total_download, source_breakdown_json, computed_at \
             FROM traffic_daily_snapshots \
             WHERE date >= ? AND date <= ? \
             ORDER BY date ASC",
        )
        .bind(start_date)
        .bind(end_date)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        rows.into_iter().map(row_to_domain).collect()
    }
}
