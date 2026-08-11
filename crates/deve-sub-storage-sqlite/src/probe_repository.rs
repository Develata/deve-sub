//! SQLite implementation of probe repositories: probe source, latency
//! record, and probe run.
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` for the probe
//! domain model.

use async_trait::async_trait;
use deve_sub_domain::{
    ErrorClass, LatencyRecord, LatencyRecordRepository, ProbeError, ProbeRun, ProbeRunRepository,
    ProbeRunResult, ProbeRunStatus, ProbeSource, ProbeSourceKind, ProbeSourceRepository, ProbeType,
};
use deve_sub_kernel::{NodeId, ProbeRunId, ProbeSourceId, Timestamp};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

/// SQLite-backed probe source repository.
pub struct SqliteProbeSourceRepository {
    pool: SqlitePool,
}

impl SqliteProbeSourceRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct ProbeSourceRow {
    id: String,
    kind: String,
    name: String,
    endpoint_url: String,
    auth_config: String,
    subscription_id: Option<String>,
    enabled: i64,
    last_sync_at: Option<String>,
    last_sync_status_kind: Option<String>,
    last_sync_status: Option<String>,
    last_counter_snapshot: Option<String>,
    created_at: String,
    updated_at: String,
}

fn row_to_source(row: ProbeSourceRow) -> Result<ProbeSource, ProbeError> {
    let kind = ProbeSourceKind::from_db_char(&row.kind)
        .ok_or_else(|| ProbeError::Storage(format!("unknown probe kind '{}'", row.kind)))?;
    let sync_status = match row.last_sync_status_kind.as_deref() {
        Some("Ok") => Some(deve_sub_domain::SyncStatus::Ok),
        Some("Failed") => Some(deve_sub_domain::SyncStatus::Failed(
            row.last_sync_status.unwrap_or_default(),
        )),
        Some("Stale") => Some(deve_sub_domain::SyncStatus::Stale),
        _ => None,
    };
    let subscription_id = row
        .subscription_id
        .map(|s| {
            deve_sub_kernel::SubscriptionId::parse(&s)
                .map_err(|e| ProbeError::Storage(format!("invalid subscription_id: {e}")))
        })
        .transpose()?;
    let id = ProbeSourceId::parse(&row.id)
        .map_err(|e| ProbeError::Storage(format!("invalid probe source id: {e}")))?;
    let created_at = parse_ts(&row.created_at).map_err(ProbeError::Storage)?;
    let updated_at = parse_ts(&row.updated_at).map_err(ProbeError::Storage)?;
    let last_sync_at = row
        .last_sync_at
        .as_deref()
        .map(parse_ts)
        .transpose()
        .map_err(ProbeError::Storage)?;
    Ok(ProbeSource {
        id,
        kind,
        name: row.name,
        endpoint_url: row.endpoint_url,
        auth_config: row.auth_config,
        subscription_id,
        enabled: row.enabled != 0,
        last_sync_at,
        last_sync_status: sync_status,
        last_counter_snapshot: row.last_counter_snapshot,
        created_at,
        updated_at,
    })
}

#[async_trait]
impl ProbeSourceRepository for SqliteProbeSourceRepository {
    async fn create(&self, source: &ProbeSource) -> Result<(), ProbeError> {
        let created_at = format_ts(source.created_at).map_err(ProbeError::Storage)?;
        let updated_at = format_ts(source.updated_at).map_err(ProbeError::Storage)?;
        let last_sync_at = source
            .last_sync_at
            .map(format_ts)
            .transpose()
            .map_err(ProbeError::Storage)?;
        let (status_kind, status_msg) = match &source.last_sync_status {
            Some(deve_sub_domain::SyncStatus::Ok) => (Some("Ok"), None),
            Some(deve_sub_domain::SyncStatus::Failed(msg)) => (Some("Failed"), Some(msg.clone())),
            Some(deve_sub_domain::SyncStatus::Stale) => (Some("Stale"), None),
            None => (None, None),
        };
        sqlx::query(
            "INSERT INTO probe_sources \
             (id, kind, name, endpoint_url, auth_config, subscription_id, enabled, \
              last_sync_at, last_sync_status_kind, last_sync_status, last_counter_snapshot, \
              created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(source.id.to_string())
        .bind(source.kind.as_db_char())
        .bind(&source.name)
        .bind(&source.endpoint_url)
        .bind(&source.auth_config)
        .bind(source.subscription_id.map(|id| id.to_string()))
        .bind(i64::from(source.enabled))
        .bind(last_sync_at)
        .bind(status_kind)
        .bind(status_msg)
        .bind(&source.last_counter_snapshot)
        .bind(created_at)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                ProbeError::NameExists
            } else {
                ProbeError::Storage(e.to_string())
            }
        })?;
        Ok(())
    }

    async fn find_by_id(&self, id: ProbeSourceId) -> Result<Option<ProbeSource>, ProbeError> {
        let row: Option<ProbeSourceRow> = sqlx::query_as(
            "SELECT id, kind, name, endpoint_url, auth_config, subscription_id, enabled, \
             last_sync_at, last_sync_status_kind, last_sync_status, last_counter_snapshot, \
             created_at, updated_at FROM probe_sources WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ProbeError::Storage(e.to_string()))?;
        row.map(row_to_source).transpose()
    }

    async fn list(
        &self,
        cursor: Option<ProbeSourceId>,
        limit: u32,
        kind: Option<ProbeSourceKind>,
    ) -> Result<Vec<ProbeSource>, ProbeError> {
        let kind_char = kind.map(|k| k.as_db_char().to_owned());
        let rows: Vec<ProbeSourceRow> = if let Some(c) = cursor {
            if let Some(k) = kind_char {
                sqlx::query_as(
                    "SELECT id, kind, name, endpoint_url, auth_config, subscription_id, enabled, \
                     last_sync_at, last_sync_status_kind, last_sync_status, last_counter_snapshot, \
                     created_at, updated_at FROM probe_sources \
                     WHERE id > ? AND kind = ? ORDER BY id LIMIT ?",
                )
                .bind(c.to_string())
                .bind(k)
                .bind(i64::from(limit))
                .fetch_all(&self.pool)
                .await
            } else {
                sqlx::query_as(
                    "SELECT id, kind, name, endpoint_url, auth_config, subscription_id, enabled, \
                     last_sync_at, last_sync_status_kind, last_sync_status, last_counter_snapshot, \
                     created_at, updated_at FROM probe_sources \
                     WHERE id > ? ORDER BY id LIMIT ?",
                )
                .bind(c.to_string())
                .bind(i64::from(limit))
                .fetch_all(&self.pool)
                .await
            }
        } else if let Some(k) = kind_char {
            sqlx::query_as(
                "SELECT id, kind, name, endpoint_url, auth_config, subscription_id, enabled, \
                 last_sync_at, last_sync_status_kind, last_sync_status, last_counter_snapshot, \
                 created_at, updated_at FROM probe_sources \
                 WHERE kind = ? ORDER BY id LIMIT ?",
            )
            .bind(k)
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as(
                "SELECT id, kind, name, endpoint_url, auth_config, subscription_id, enabled, \
                 last_sync_at, last_sync_status_kind, last_sync_status, last_counter_snapshot, \
                 created_at, updated_at FROM probe_sources \
                 ORDER BY id LIMIT ?",
            )
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| ProbeError::Storage(e.to_string()))?;
        rows.into_iter().map(row_to_source).collect()
    }

    async fn update(&self, source: &ProbeSource) -> Result<(), ProbeError> {
        let updated_at = format_ts(source.updated_at).map_err(ProbeError::Storage)?;
        let last_sync_at = source
            .last_sync_at
            .map(format_ts)
            .transpose()
            .map_err(ProbeError::Storage)?;
        let (status_kind, status_msg) = match &source.last_sync_status {
            Some(deve_sub_domain::SyncStatus::Ok) => (Some("Ok"), None),
            Some(deve_sub_domain::SyncStatus::Failed(msg)) => (Some("Failed"), Some(msg.clone())),
            Some(deve_sub_domain::SyncStatus::Stale) => (Some("Stale"), None),
            None => (None, None),
        };
        let result = sqlx::query(
            "UPDATE probe_sources SET kind = ?, name = ?, endpoint_url = ?, auth_config = ?, \
             subscription_id = ?, enabled = ?, last_sync_at = ?, last_sync_status_kind = ?, \
             last_sync_status = ?, last_counter_snapshot = ?, updated_at = ? WHERE id = ?",
        )
        .bind(source.kind.as_db_char())
        .bind(&source.name)
        .bind(&source.endpoint_url)
        .bind(&source.auth_config)
        .bind(source.subscription_id.map(|id| id.to_string()))
        .bind(i64::from(source.enabled))
        .bind(last_sync_at)
        .bind(status_kind)
        .bind(status_msg)
        .bind(&source.last_counter_snapshot)
        .bind(updated_at)
        .bind(source.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                ProbeError::NameExists
            } else {
                ProbeError::Storage(e.to_string())
            }
        })?;
        if result.rows_affected() == 0 {
            return Err(ProbeError::SourceNotFound);
        }
        Ok(())
    }

    async fn delete(&self, id: ProbeSourceId) -> Result<(), ProbeError> {
        let result = sqlx::query("DELETE FROM probe_sources WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| ProbeError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(ProbeError::SourceNotFound);
        }
        Ok(())
    }
}

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
    let probe_type = ProbeType::from_db_char(&row.probe_type)
        .ok_or_else(|| ProbeError::Storage(format!("unknown probe type '{}'", row.probe_type)))?;
    let error_class = row
        .error_class
        .as_deref()
        .map(|c| {
            ErrorClass::from_db_char(c)
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
            Some(record.error_class.as_db_char())
        };
        sqlx::query(
            "INSERT INTO latency_records \
             (id, run_id, node_id, probe_type, rtt_ms, error_class, measured_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(record.id.to_string())
        .bind(record.run_id.to_string())
        .bind(record.node_id.to_string())
        .bind(record.probe_type.as_db_char())
        .bind(record.rtt_ms.map(i64::from))
        .bind(error_class)
        .bind(measured_at)
        .execute(&self.pool)
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

/// SQLite-backed probe run repository.
pub struct SqliteProbeRunRepository {
    pool: SqlitePool,
}

impl SqliteProbeRunRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(Serialize, Deserialize)]
struct ResultJson {
    node_id: String,
    rtt_ms: Option<u32>,
    error_class: Option<String>,
    skipped: bool,
}

#[derive(sqlx::FromRow)]
struct ProbeRunRow {
    id: String,
    probe_type: String,
    node_ids: String,
    status: String,
    results: String,
    created_at: String,
    completed_at: Option<String>,
}

fn row_to_run(row: ProbeRunRow) -> Result<ProbeRun, ProbeError> {
    let id = ProbeRunId::parse(&row.id)
        .map_err(|e| ProbeError::Storage(format!("invalid probe run id: {e}")))?;
    let probe_type = ProbeType::from_db_char(&row.probe_type)
        .ok_or_else(|| ProbeError::Storage(format!("unknown probe type '{}'", row.probe_type)))?;
    let status = ProbeRunStatus::from_db_char(&row.status)
        .ok_or_else(|| ProbeError::Storage(format!("unknown run status '{}'", row.status)))?;
    let node_ids: Vec<String> = serde_json::from_str(&row.node_ids)
        .map_err(|e| ProbeError::Storage(format!("invalid node_ids JSON: {e}")))?;
    let node_ids: Vec<NodeId> = node_ids
        .iter()
        .map(|s| NodeId::parse(s))
        .collect::<Result<_, _>>()
        .map_err(|e| ProbeError::Storage(format!("invalid node id in run: {e}")))?;
    let results_json: Vec<ResultJson> = serde_json::from_str(&row.results)
        .map_err(|e| ProbeError::Storage(format!("invalid results JSON: {e}")))?;
    let results: Vec<ProbeRunResult> = results_json
        .into_iter()
        .map(|r| {
            let node_id = NodeId::parse(&r.node_id)
                .map_err(|e| ProbeError::Storage(format!("invalid node id in result: {e}")))?;
            let error_class = r
                .error_class
                .as_deref()
                .map(|c| {
                    ErrorClass::from_db_char(c)
                        .ok_or_else(|| ProbeError::Storage(format!("unknown error class '{c}'")))
                })
                .transpose()?
                .unwrap_or(ErrorClass::Ok);
            Ok(ProbeRunResult {
                node_id,
                rtt_ms: r.rtt_ms,
                error_class,
                skipped: r.skipped,
            })
        })
        .collect::<Result<_, _>>()?;
    let created_at = parse_ts(&row.created_at).map_err(ProbeError::Storage)?;
    let completed_at = row
        .completed_at
        .as_deref()
        .map(parse_ts)
        .transpose()
        .map_err(ProbeError::Storage)?;
    Ok(ProbeRun {
        id,
        probe_type,
        node_ids,
        status,
        results,
        created_at,
        completed_at,
    })
}

#[async_trait]
impl ProbeRunRepository for SqliteProbeRunRepository {
    async fn create(&self, run: &ProbeRun) -> Result<(), ProbeError> {
        let created_at = format_ts(run.created_at).map_err(ProbeError::Storage)?;
        let node_ids: Vec<String> = run.node_ids.iter().map(|id| id.to_string()).collect();
        let node_ids_json = serde_json::to_string(&node_ids)
            .map_err(|e| ProbeError::Storage(format!("node_ids serialize: {e}")))?;
        let results: Vec<ResultJson> = run
            .results
            .iter()
            .map(|r| {
                let error_class = if r.error_class == ErrorClass::Ok {
                    None
                } else {
                    Some(r.error_class.as_db_char().to_owned())
                };
                ResultJson {
                    node_id: r.node_id.to_string(),
                    rtt_ms: r.rtt_ms,
                    error_class,
                    skipped: r.skipped,
                }
            })
            .collect();
        let results_json = serde_json::to_string(&results)
            .map_err(|e| ProbeError::Storage(format!("results serialize: {e}")))?;
        sqlx::query(
            "INSERT INTO probe_runs (id, probe_type, node_ids, status, results, created_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(run.id.to_string())
        .bind(run.probe_type.as_db_char())
        .bind(node_ids_json)
        .bind(run.status.as_db_char())
        .bind(results_json)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| ProbeError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_by_id(&self, id: ProbeRunId) -> Result<Option<ProbeRun>, ProbeError> {
        let row: Option<ProbeRunRow> = sqlx::query_as(
            "SELECT id, probe_type, node_ids, status, results, created_at, completed_at \
             FROM probe_runs WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ProbeError::Storage(e.to_string()))?;
        row.map(row_to_run).transpose()
    }

    async fn update_status(
        &self,
        id: ProbeRunId,
        status: ProbeRunStatus,
        results: &[ProbeRunResult],
        completed_at: Option<Timestamp>,
    ) -> Result<(), ProbeError> {
        let results_json: Vec<ResultJson> = results
            .iter()
            .map(|r| {
                let error_class = if r.error_class == ErrorClass::Ok {
                    None
                } else {
                    Some(r.error_class.as_db_char().to_owned())
                };
                ResultJson {
                    node_id: r.node_id.to_string(),
                    rtt_ms: r.rtt_ms,
                    error_class,
                    skipped: r.skipped,
                }
            })
            .collect();
        let results_str = serde_json::to_string(&results_json)
            .map_err(|e| ProbeError::Storage(format!("results serialize: {e}")))?;
        let completed_str = completed_at
            .map(format_ts)
            .transpose()
            .map_err(ProbeError::Storage)?;
        let result = sqlx::query(
            "UPDATE probe_runs SET status = ?, results = ?, completed_at = ? WHERE id = ?",
        )
        .bind(status.as_db_char())
        .bind(results_str)
        .bind(completed_str)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| ProbeError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(ProbeError::RunNotFound);
        }
        Ok(())
    }

    async fn recover_crashed_runs(&self) -> Result<u64, ProbeError> {
        let result =
            sqlx::query("UPDATE probe_runs SET status = 'F' WHERE status = 'R' OR status = 'P'")
                .execute(&self.pool)
                .await
                .map_err(|e| ProbeError::Storage(e.to_string()))?;
        Ok(result.rows_affected())
    }

    async fn delete(&self, id: ProbeRunId) -> Result<(), ProbeError> {
        let result = sqlx::query("DELETE FROM probe_runs WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| ProbeError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(ProbeError::RunNotFound);
        }
        Ok(())
    }
}
