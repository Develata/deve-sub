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

    async fn get_summary_in_range(
        &self,
        subscription_id: SubscriptionId,
        start_iso: &str,
        end_iso: &str,
    ) -> Result<TrafficSummary, SubscriptionError> {
        let rows: Vec<AggregateRow> = sqlx::query_as(
            "SELECT source_kind, SUM(upload) AS upload, SUM(download) AS download \
             FROM subscription_traffic \
             WHERE subscription_id = ? AND recorded_at >= ? AND recorded_at < ? \
             GROUP BY source_kind",
        )
        .bind(subscription_id.to_string())
        .bind(start_iso)
        .bind(end_iso)
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

    async fn get_global_summary(&self) -> Result<TrafficSummary, SubscriptionError> {
        let rows: Vec<AggregateRow> = sqlx::query_as(
            "SELECT source_kind, SUM(upload) AS upload, SUM(download) AS download \
             FROM subscription_traffic GROUP BY source_kind",
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
        sqlx::query("DELETE FROM subscription_traffic WHERE subscription_id = ?")
            .bind(subscription_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn get_probe_traffic_attributions(
        &self,
    ) -> Result<Vec<(SubscriptionId, String, u64, u64)>, SubscriptionError> {
        // WHY: source_ref shape is "{kind_kebab}:{external_server_id}".
        // substr(..., 1, instr(..., ':') - 1) extracts the kind prefix; the
        // -1 removes the trailing ':'; if there is no ':' the prefix is the
        // full value (lenient — non-probe refs without ':' fall here too,
        // but the WHERE clause restricts to source_kind = 'p' (Probe)).
        let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT subscription_id, \
                    CASE WHEN instr(source_ref, ':') > 0 \
                         THEN substr(source_ref, 1, instr(source_ref, ':') - 1) \
                         ELSE source_ref END AS prefix, \
                    SUM(upload) AS upload, SUM(download) AS download \
             FROM subscription_traffic \
             WHERE source_kind = 'P' \
             GROUP BY subscription_id, prefix",
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

    async fn subscriptions_with_traffic_in_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<SubscriptionId>, SubscriptionError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT subscription_id FROM subscription_traffic \
             WHERE recorded_at >= ? AND recorded_at < ?",
        )
        .bind(start_date)
        .bind(end_date)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        rows.into_iter()
            .map(|(s,)| {
                SubscriptionId::parse(&s).map_err(|e| {
                    SubscriptionError::Storage(format!("invalid subscription id: {e}"))
                })
            })
            .collect()
    }
}
