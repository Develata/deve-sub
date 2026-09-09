//! Latency sample persistence.
use crate::{
    discriminant::SqliteDiscriminant,
    timestamp::{format_ts, parse_ts},
};
use async_trait::async_trait;
use deve_sub_domain::{ErrorClass, LatencyRecord, LatencyRecordRepository, ProbeError, ProbeType};
use deve_sub_kernel::{NodeId, ProbeRunId};
use sqlx::SqlitePool;
/// SQLite-backed latency record repository.
pub struct SqliteLatencyRecordRepository {
    pool: SqlitePool,
}

impl SqliteLatencyRecordRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct LatencyRecordRow {
    id: String,
    run_id: String,
    node_id: String,
    probe_type: String,
    rtt_ms: Option<i64>,
    error_class: Option<String>,
    measured_at: String,
}

fn row_to_record(row: LatencyRecordRow) -> Result<LatencyRecord, ProbeError> {
    let id = deve_sub_kernel::LatencyRecordId::parse(&row.id)
        .map_err(|e| ProbeError::Storage(format!("invalid latency record id: {e}")))?;
    let run_id = ProbeRunId::parse(&row.run_id)
        .map_err(|e| ProbeError::Storage(format!("invalid probe run id: {e}")))?;
    let node_id = NodeId::parse(&row.node_id)
        .map_err(|e| ProbeError::Storage(format!("invalid node id: {e}")))?;
    let probe_type = ProbeType::decode(&row.probe_type)
        .ok_or_else(|| ProbeError::Storage(format!("unknown probe type '{}'", row.probe_type)))?;
    let error_class = row
        .error_class
        .as_deref()
        .map(|c| {
            ErrorClass::decode(c)
                .ok_or_else(|| ProbeError::Storage(format!("unknown error class '{c}'")))
        })
        .transpose()?
        .unwrap_or(ErrorClass::Ok);
    let measured_at = parse_ts(&row.measured_at).map_err(ProbeError::Storage)?;
    Ok(LatencyRecord {
        id,
        run_id,
        node_id,
        probe_type,
        rtt_ms: row.rtt_ms.map(|v| v.max(0) as u32),
        error_class,
        measured_at,
    })
}

#[async_trait]
impl LatencyRecordRepository for SqliteLatencyRecordRepository {
    async fn create(&self, record: &LatencyRecord) -> Result<(), ProbeError> {
        let measured_at = format_ts(record.measured_at).map_err(ProbeError::Storage)?;
        let error_class = if record.error_class == ErrorClass::Ok {
            None
        } else {
            Some(record.error_class.encode())
        };
        sqlx::query(
            "INSERT INTO latency_records \
             (id, run_id, node_id, probe_type, rtt_ms, error_class, measured_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(record.id.to_string())
        .bind(record.run_id.to_string())
        .bind(record.node_id.to_string())
        .bind(record.probe_type.encode())
        .bind(record.rtt_ms.map(i64::from))
        .bind(error_class)
        .bind(measured_at)
        .execute(&self.pool)
        .await
        .map_err(|e| ProbeError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn batch_create(&self, records: &[LatencyRecord]) -> Result<(), ProbeError> {
        if records.is_empty() {
            return Ok(());
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| ProbeError::Storage(e.to_string()))?;
        for record in records {
            let measured_at = format_ts(record.measured_at).map_err(ProbeError::Storage)?;
            let error_class = if record.error_class == ErrorClass::Ok {
                None
            } else {
                Some(record.error_class.encode())
            };
            sqlx::query(
                "INSERT INTO latency_records \
                 (id, run_id, node_id, probe_type, rtt_ms, error_class, measured_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(record.id.to_string())
            .bind(record.run_id.to_string())
            .bind(record.node_id.to_string())
            .bind(record.probe_type.encode())
            .bind(record.rtt_ms.map(i64::from))
            .bind(error_class)
            .bind(measured_at)
            .execute(&mut *tx)
            .await
            .map_err(|e| ProbeError::Storage(e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| ProbeError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn list_for_node(
        &self,
        node_id: NodeId,
        limit: u32,
    ) -> Result<Vec<LatencyRecord>, ProbeError> {
        let rows: Vec<LatencyRecordRow> = sqlx::query_as(
            "SELECT id, run_id, node_id, probe_type, rtt_ms, error_class, measured_at \
             FROM latency_records WHERE node_id = ? ORDER BY measured_at DESC LIMIT ?",
        )
        .bind(node_id.to_string())
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ProbeError::Storage(e.to_string()))?;
        rows.into_iter().map(row_to_record).collect()
    }

    async fn list_recent(&self, limit: u32) -> Result<Vec<LatencyRecord>, ProbeError> {
        let rows: Vec<LatencyRecordRow> = sqlx::query_as(
            "SELECT id, run_id, node_id, probe_type, rtt_ms, error_class, measured_at \
             FROM latency_records ORDER BY measured_at DESC LIMIT ?",
        )
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ProbeError::Storage(e.to_string()))?;
        rows.into_iter().map(row_to_record).collect()
    }

    async fn delete_for_run(&self, run_id: ProbeRunId) -> Result<(), ProbeError> {
        sqlx::query("DELETE FROM latency_records WHERE run_id = ?")
            .bind(run_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| ProbeError::Storage(e.to_string()))?;
        Ok(())
    }
}
