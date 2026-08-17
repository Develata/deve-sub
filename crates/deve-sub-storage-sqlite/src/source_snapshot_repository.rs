//! SQLite implementation of [`SourceSnapshotRepository`].
//!
//! Creating a new snapshot deactivates the previous active snapshot for the
//! same source in a single transaction, so there is never a window where
//! a source has zero or two active snapshots. See ADR-0002.

use async_trait::async_trait;
use deve_sub_domain::{SourceError, SourceSnapshot, SourceSnapshotRepository};
use deve_sub_kernel::{SourceId, SourceSnapshotId};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

/// SQLite-backed source snapshot repository.
pub struct SqliteSourceSnapshotRepository {
    pool: SqlitePool,
}

impl SqliteSourceSnapshotRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Internal row representation for `sqlx::FromRow`.
#[derive(sqlx::FromRow)]
struct SnapshotRow {
    id: String,
    source_id: String,
    version: i64,
    fetched_at: String,
    etag: Option<String>,
    node_count: i64,
    is_active: i64,
}

impl SnapshotRow {
    fn to_domain(&self) -> Result<SourceSnapshot, SourceError> {
        Ok(SourceSnapshot {
            id: SourceSnapshotId::parse(&self.id)
                .map_err(|e| SourceError::Storage(e.to_string()))?,
            source_id: SourceId::parse(&self.source_id)
                .map_err(|e| SourceError::Storage(e.to_string()))?,
            version: u64::try_from(self.version)
                .map_err(|_| SourceError::Storage("negative version".to_owned()))?,
            fetched_at: parse_ts(&self.fetched_at).map_err(SourceError::Storage)?,
            etag: self.etag.clone(),
            node_count: u64::try_from(self.node_count)
                .map_err(|_| SourceError::Storage("negative node_count".to_owned()))?,
            is_active: self.is_active != 0,
        })
    }
}

#[async_trait]
impl SourceSnapshotRepository for SqliteSourceSnapshotRepository {
    async fn create(&self, snapshot: &SourceSnapshot) -> Result<(), SourceError> {
        // WHY: deactivate-then-insert in a single transaction so there is
        // never a window where a source has two active snapshots or zero
        // active snapshots between the deactivation and the insert.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;

        sqlx::query(
            "UPDATE source_snapshots SET is_active = 0 WHERE source_id = ? AND is_active = 1",
        )
        .bind(snapshot.source_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;

        let fetched_at = format_ts(snapshot.fetched_at).map_err(SourceError::Storage)?;
        sqlx::query(
            "INSERT INTO source_snapshots (id, source_id, version, fetched_at, etag, node_count, is_active) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(snapshot.id.to_string())
        .bind(snapshot.source_id.to_string())
        .bind(snapshot.version as i64)
        .bind(fetched_at)
        .bind(&snapshot.etag)
        .bind(snapshot.node_count as i64)
        .bind(snapshot.is_active as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_active(
        &self,
        source_id: SourceId,
    ) -> Result<Option<SourceSnapshot>, SourceError> {
        let row: Option<SnapshotRow> = sqlx::query_as(
            "SELECT id, source_id, version, fetched_at, etag, node_count, is_active \
             FROM source_snapshots \
             WHERE source_id = ? AND is_active = 1",
        )
        .bind(source_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn find_active_for_sources(
        &self,
        source_ids: &[SourceId],
    ) -> Result<std::collections::HashMap<SourceId, SourceSnapshot>, SourceError> {
        if source_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let placeholders = std::iter::repeat_n("?,", source_ids.len())
            .collect::<String>()
            .trim_end_matches(',')
            .to_owned();
        let sql = format!(
            "SELECT id, source_id, version, fetched_at, etag, node_count, is_active \
             FROM source_snapshots \
             WHERE is_active = 1 AND source_id IN ({placeholders})"
        );
        let mut query = sqlx::query_as::<_, SnapshotRow>(&sql);
        for id in source_ids {
            query = query.bind(id.to_string());
        }
        let rows: Vec<SnapshotRow> = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        let mut map = std::collections::HashMap::with_capacity(rows.len());
        for row in rows {
            let snapshot = row.to_domain()?;
            map.insert(snapshot.source_id, snapshot);
        }
        Ok(map)
    }

    async fn list_for_source(
        &self,
        source_id: SourceId,
        limit: u32,
    ) -> Result<Vec<SourceSnapshot>, SourceError> {
        let limit = limit.min(100) as i64;
        let rows: Vec<SnapshotRow> = sqlx::query_as(
            "SELECT id, source_id, version, fetched_at, etag, node_count, is_active \
             FROM source_snapshots \
             WHERE source_id = ? \
             ORDER BY version DESC \
             LIMIT ?",
        )
        .bind(source_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        rows.iter().map(|r| r.to_domain()).collect()
    }

    async fn find_by_id(
        &self,
        id: SourceSnapshotId,
    ) -> Result<Option<SourceSnapshot>, SourceError> {
        let row: Option<SnapshotRow> = sqlx::query_as(
            "SELECT id, source_id, version, fetched_at, etag, node_count, is_active \
             FROM source_snapshots \
             WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }
}
