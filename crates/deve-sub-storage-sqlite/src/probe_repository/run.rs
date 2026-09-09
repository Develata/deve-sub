//! Probe run state transitions and crash recovery.
use crate::{
    discriminant::SqliteDiscriminant,
    timestamp::{format_ts, parse_ts},
};
use async_trait::async_trait;
use deve_sub_domain::{
    ErrorClass, ProbeError, ProbeRun, ProbeRunRepository, ProbeRunResult, ProbeRunStatus, ProbeType,
};
use deve_sub_kernel::{NodeId, ProbeRunId, Timestamp};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
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
    let probe_type = ProbeType::decode(&row.probe_type)
        .ok_or_else(|| ProbeError::Storage(format!("unknown probe type '{}'", row.probe_type)))?;
    let status = ProbeRunStatus::decode(&row.status)
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
                    ErrorClass::decode(c)
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
                    Some(r.error_class.encode().to_owned())
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
        .bind(run.probe_type.encode())
        .bind(node_ids_json)
        .bind(run.status.encode())
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
                    Some(r.error_class.encode().to_owned())
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
        // WHY: the guard blocks overwriting a terminal row with a DIFFERENT
        // terminal status (e.g. cancel wrote `Cancelled`, runner tries
        // `Completed`). But it ALLOWS idempotent same-status writes
        // (`Cancelled` → `Cancelled`) so the runner can persist its collected
        // results onto a row that cancel already flipped to `Cancelled` (W-F).
        let status_char = status.encode();
        let result = sqlx::query(
            "UPDATE probe_runs SET status = ?, results = ?, completed_at = ? \
             WHERE id = ? AND (status NOT IN ('C', 'X', 'F') OR status = ?)",
        )
        .bind(status_char)
        .bind(results_str)
        .bind(completed_str)
        .bind(id.to_string())
        .bind(status_char)
        .execute(&self.pool)
        .await
        .map_err(|e| ProbeError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            // WHY: 0 rows can mean either (a) the run never existed, or (b)
            // a concurrent cancel marked the row terminal between our last
            // read and this UPDATE (W-F race). Distinguish via a follow-up
            // SELECT so the runner can treat a cancel-win as Ok rather than
            // a spurious RunNotFound.
            let existing: Option<(String,)> =
                sqlx::query_as("SELECT id FROM probe_runs WHERE id = ?")
                    .bind(id.to_string())
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| ProbeError::Storage(e.to_string()))?;
            if existing.is_none() {
                return Err(ProbeError::RunNotFound);
            }
            return Err(ProbeError::RunAlreadyTerminal);
        }
        Ok(())
    }

    async fn update_results(
        &self,
        id: ProbeRunId,
        results: &[ProbeRunResult],
        completed_at: Option<Timestamp>,
    ) -> Result<(), ProbeError> {
        let results_json: Vec<ResultJson> = results
            .iter()
            .map(|r| {
                let error_class = if r.error_class == ErrorClass::Ok {
                    None
                } else {
                    Some(r.error_class.encode().to_owned())
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
        let result =
            sqlx::query("UPDATE probe_runs SET results = ?, completed_at = ? WHERE id = ?")
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
        // WHY: recovery starts the diagnostic retention window; a NULL
        // completion time would make failed runs immune to maintenance.
        let result = sqlx::query(
            "UPDATE probe_runs SET status = 'F', completed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') \
             WHERE status IN ('R', 'P')",
        )
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
