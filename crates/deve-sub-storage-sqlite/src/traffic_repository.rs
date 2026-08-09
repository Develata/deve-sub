//! SQLite implementation of [`TrafficRepository`].
//!
//! Traffic records are append-only observations per subscription. Aggregation
//! (sum of upload/download, optionally grouped by source_kind) is computed at
//! read time. See `docs/plan/milestones/M6-subscription-distribution.md`
//! §"Traffic and expiry policy framework".

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
        let kind = TrafficSourceKind::from_db_char(&row.source_kind).ok_or_else(|| {
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
        .bind(record.source_kind.as_db_char())
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
            "SELECT source_kind, SUM(upload) AS upload, SUM(download) AS download \
             FROM subscription_traffic WHERE subscription_id = ? \
             GROUP BY source_kind",
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
             FROM subscription_traffic t \
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

    async fn delete_for_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<(), SubscriptionError> {
        sqlx::query("DELETE FROM subscription_traffic WHERE subscription_id = ?")
            .bind(subscription_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        Ok(())
    }
}
