//! SQLite implementation of [`TrafficRepository`].
//!
//! Raw deltas and lifetime/daily projections commit together via migration
//! 0024 triggers. Lifetime queries never scan retained raw history.

use crate::discriminant::SqliteDiscriminant;
use async_trait::async_trait;
use deve_sub_domain::{
    SubscriptionError, TrafficRecord, TrafficRepository, TrafficSourceKind, TrafficSummary,
};
use deve_sub_kernel::{SubscriptionId, UserId};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::format_ts;

/// SQLite-backed traffic repository.
pub struct SqliteTrafficRepository {
    pool: SqlitePool,
}

impl SqliteTrafficRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct AggregateRow {
    source_kind: String,
    upload: i64,
    download: i64,
}

fn build_summary(rows: Vec<AggregateRow>) -> Result<TrafficSummary, SubscriptionError> {
    let mut upload: u64 = 0;
    let mut download: u64 = 0;
    let mut by_source: Vec<(TrafficSourceKind, u64, u64)> = Vec::new();
    for row in rows {
        let kind = TrafficSourceKind::decode(&row.source_kind).ok_or_else(|| {
            SubscriptionError::Storage(format!("unknown source_kind '{}'", row.source_kind))
        })?;
        let u = row.upload.max(0) as u64;
        let d = row.download.max(0) as u64;
        upload = upload.saturating_add(u);
        download = download.saturating_add(d);
        by_source.push((kind, u, d));
    }
    Ok(TrafficSummary {
        upload,
        download,
        by_source,
    })
}

#[async_trait]
impl TrafficRepository for SqliteTrafficRepository {
    async fn create(&self, record: &TrafficRecord) -> Result<(), SubscriptionError> {
        let recorded_at = format_ts(record.recorded_at).map_err(SubscriptionError::Storage)?;
        sqlx::query(
            "INSERT INTO subscription_traffic \
             (id, subscription_id, source_kind, upload, download, recorded_at, source_ref) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(record.id.to_string())
        .bind(record.subscription_id.to_string())
        .bind(record.source_kind.encode())
        .bind(record.upload as i64)
        .bind(record.download as i64)
        .bind(recorded_at)
        .bind(&record.source_ref)
        .execute(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn get_summary(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<TrafficSummary, SubscriptionError> {
        let rows: Vec<AggregateRow> = sqlx::query_as(
            "SELECT source_kind, upload, download \
             FROM traffic_totals WHERE subscription_id = ? ORDER BY source_kind",
        )
        .bind(subscription_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        build_summary(rows)
    }

    async fn get_summary_for_user(
        &self,
        user_id: UserId,
    ) -> Result<TrafficSummary, SubscriptionError> {
        let rows: Vec<AggregateRow> = sqlx::query_as(
            "SELECT t.source_kind, SUM(t.upload) AS upload, SUM(t.download) AS download \
             FROM traffic_totals t \
             INNER JOIN subscriptions s ON t.subscription_id = s.id \
             WHERE s.owner_id = ? \
             GROUP BY t.source_kind",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        build_summary(rows)
    }

    async fn get_global_summary(&self) -> Result<TrafficSummary, SubscriptionError> {
        let rows: Vec<AggregateRow> = sqlx::query_as(
            "SELECT source_kind, SUM(upload) AS upload, SUM(download) AS download \
             FROM traffic_totals GROUP BY source_kind",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        build_summary(rows)
    }

    async fn delete_for_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<(), SubscriptionError> {
        // WHY: explicit reset must remove every projection in the same commit.
        // Retention uses raw-only deletes and intentionally preserves totals.
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        for table in [
            "subscription_traffic",
            "traffic_totals",
            "probe_traffic_totals",
            "traffic_daily_snapshots",
        ] {
            sqlx::query(&format!("DELETE FROM {table} WHERE subscription_id = ?"))
                .bind(subscription_id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(storage_error)?;
        }
        tx.commit().await.map_err(storage_error)?;
        Ok(())
    }

    async fn get_probe_traffic_attributions(
        &self,
    ) -> Result<Vec<(SubscriptionId, String, u64, u64)>, SubscriptionError> {
        let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT subscription_id, prefix, upload, download FROM probe_traffic_totals \
             ORDER BY subscription_id, prefix",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;

        let mut out = Vec::with_capacity(rows.len());
        for (sub_id_str, prefix, up, down) in rows {
            let sub_id = SubscriptionId::parse(&sub_id_str).map_err(|e| {
                SubscriptionError::Storage(format!("invalid subscription id '{sub_id_str}': {e}"))
            })?;
            out.push((sub_id, prefix, up.max(0) as u64, down.max(0) as u64));
        }
        Ok(out)
    }
}

fn storage_error(error: sqlx::Error) -> SubscriptionError {
    SubscriptionError::Storage(error.to_string())
}
