//! SQLite implementation of probe repositories: probe source, latency
//! record, and probe run.
//!
//! Sensitive fields (`auth_config`, `last_counter_snapshot`) are encrypted
//! at rest with XChaCha20-Poly1305 via the v2 secret envelope (HKDF subkey
//! and column-bound AAD); see ADR-0007. Encryption and decryption are
//! transparent here — the domain entity carries plaintext only.
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` for the probe
//! domain model.

use std::sync::Arc;

use async_trait::async_trait;
use deve_sub_domain::{
    ErrorClass, LatencyRecord, LatencyRecordRepository, ProbeError, ProbeRun, ProbeRunRepository,
    ProbeRunResult, ProbeRunStatus, ProbeSource, ProbeSourceKind, ProbeSourceRepository, ProbeType,
};
use deve_sub_kernel::{NodeId, ProbeRunId, ProbeSourceId, Timestamp};
use deve_sub_security::{MasterKey, envelope};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

/// HKDF/AAD context label for the probe source auth_config column.
const CTX_AUTH_CONFIG: &[u8] = b"probe_sources.auth_config";
/// HKDF/AAD context label for the probe source last_counter_snapshot column.
const CTX_COUNTER_SNAPSHOT: &[u8] = b"probe_sources.last_counter_snapshot";

/// SQLite-backed probe source repository.
pub struct SqliteProbeSourceRepository {
    pool: SqlitePool,
    master_key: Option<Arc<MasterKey>>,
}

impl SqliteProbeSourceRepository {
    /// Create a new repository without at-rest encryption.
    ///
    /// Sensitive columns will be stored as empty/NULL and cannot be read
    /// back. Use this only for tests that do not touch auth_config or
    /// counter snapshot data.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            master_key: None,
        }
    }

    /// Create a new repository with at-rest encryption.
    ///
    /// `auth_config` and `last_counter_snapshot` are encrypted with
    /// XChaCha20-Poly1305 (v2 envelope with HKDF subkey derivation and AAD)
    /// before being written to the database. See ADR-0007.
    #[must_use]
    pub fn new_with_key(pool: SqlitePool, master_key: Arc<MasterKey>) -> Self {
        Self {
            pool,
            master_key: Some(master_key),
        }
    }

    /// Encrypt a plaintext string into an envelope. Empty strings are
    /// stored as empty strings (not encrypted) so DStatus/Komari sources
    /// with no auth_config round-trip cleanly.
    fn seal_str(&self, context: &[u8], plaintext: &str) -> Result<String, ProbeError> {
        if plaintext.is_empty() {
            return Ok(String::new());
        }
        match &self.master_key {
            Some(key) => envelope::seal(key.as_bytes(), context, plaintext.as_bytes())
                .map_err(|e| ProbeError::Storage(format!("encryption failed: {e}"))),
            None => Err(ProbeError::Storage(
                "no master key — cannot encrypt sensitive column".to_owned(),
            )),
        }
    }

    /// Decrypt an envelope string. Empty strings are returned as-is (no
    /// auth_config for DStatus/Komari).
    fn open_str(&self, context: &[u8], encrypted: &str) -> Result<String, ProbeError> {
        if encrypted.is_empty() {
            return Ok(String::new());
        }
        match &self.master_key {
            Some(key) => {
                let bytes = envelope::open(key.as_bytes(), context, encrypted)
                    .map_err(|e| ProbeError::Storage(format!("decryption failed: {e}")))?;
                String::from_utf8(bytes)
                    .map_err(|e| ProbeError::Storage(format!("decrypted value is not UTF-8: {e}")))
            }
            None => Err(ProbeError::Storage(
                "no master key — cannot decrypt sensitive column".to_owned(),
            )),
        }
    }

    /// Encrypt an optional plaintext string into an optional envelope.
    /// `None` and empty strings map to `None` (NULL column).
    fn seal_opt(
        &self,
        context: &[u8],
        plaintext: &Option<String>,
    ) -> Result<Option<String>, ProbeError> {
        match plaintext {
            Some(s) if !s.is_empty() => Ok(Some(self.seal_str(context, s)?)),
            _ => Ok(None),
        }
    }

    /// Decrypt an optional envelope. Returns `None` if the column is NULL
    /// or empty; errors if a key is set but decryption fails, or if no key
    /// is set and the column is non-empty.
    fn open_opt(
        &self,
        context: &[u8],
        encrypted: &Option<String>,
    ) -> Result<Option<String>, ProbeError> {
        match encrypted {
            Some(env) if !env.is_empty() => Ok(Some(self.open_str(context, env)?)),
            _ => Ok(None),
        }
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

impl ProbeSourceRow {
    fn to_domain(&self, repo: &SqliteProbeSourceRepository) -> Result<ProbeSource, ProbeError> {
        let kind = ProbeSourceKind::from_db_char(&self.kind)
            .ok_or_else(|| ProbeError::Storage(format!("unknown probe kind '{}'", self.kind)))?;
        let sync_status = match self.last_sync_status_kind.as_deref() {
            Some("Ok") => Some(deve_sub_domain::SyncStatus::Ok),
            Some("Failed") => Some(deve_sub_domain::SyncStatus::Failed(
                self.last_sync_status.clone().unwrap_or_default(),
            )),
            Some("Stale") => Some(deve_sub_domain::SyncStatus::Stale),
            _ => None,
        };
        let subscription_id = self
            .subscription_id
            .as_ref()
            .map(|s| {
                deve_sub_kernel::SubscriptionId::parse(s)
                    .map_err(|e| ProbeError::Storage(format!("invalid subscription_id: {e}")))
            })
            .transpose()?;
        let id = ProbeSourceId::parse(&self.id)
            .map_err(|e| ProbeError::Storage(format!("invalid probe source id: {e}")))?;
        let created_at = parse_ts(&self.created_at).map_err(ProbeError::Storage)?;
        let updated_at = parse_ts(&self.updated_at).map_err(ProbeError::Storage)?;
        let last_sync_at = self
            .last_sync_at
            .as_deref()
            .map(parse_ts)
            .transpose()
            .map_err(ProbeError::Storage)?;
        let auth_config = repo.open_str(CTX_AUTH_CONFIG, &self.auth_config)?;
        let last_counter_snapshot =
            repo.open_opt(CTX_COUNTER_SNAPSHOT, &self.last_counter_snapshot)?;
        Ok(ProbeSource {
            id,
            kind,
            name: self.name.clone(),
            endpoint_url: self.endpoint_url.clone(),
            auth_config,
            subscription_id,
            enabled: self.enabled != 0,
            last_sync_at,
            last_sync_status: sync_status,
            last_counter_snapshot,
            created_at,
            updated_at,
        })
    }
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
        let auth_config_enc = self.seal_str(CTX_AUTH_CONFIG, &source.auth_config)?;
        let snapshot_enc = self.seal_opt(CTX_COUNTER_SNAPSHOT, &source.last_counter_snapshot)?;
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
        .bind(&auth_config_enc)
        .bind(source.subscription_id.map(|id| id.to_string()))
        .bind(i64::from(source.enabled))
        .bind(last_sync_at)
        .bind(status_kind)
        .bind(status_msg)
        .bind(&snapshot_enc)
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
        row.map(|r| r.to_domain(self)).transpose()
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
        rows.iter().map(|r| r.to_domain(self)).collect()
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
        let auth_config_enc = self.seal_str(CTX_AUTH_CONFIG, &source.auth_config)?;
        let snapshot_enc = self.seal_opt(CTX_COUNTER_SNAPSHOT, &source.last_counter_snapshot)?;
        let result = sqlx::query(
            "UPDATE probe_sources SET kind = ?, name = ?, endpoint_url = ?, auth_config = ?, \
             subscription_id = ?, enabled = ?, last_sync_at = ?, last_sync_status_kind = ?, \
             last_sync_status = ?, last_counter_snapshot = ?, updated_at = ? WHERE id = ?",
        )
        .bind(source.kind.as_db_char())
        .bind(&source.name)
        .bind(&source.endpoint_url)
        .bind(&auth_config_enc)
        .bind(source.subscription_id.map(|id| id.to_string()))
        .bind(i64::from(source.enabled))
        .bind(last_sync_at)
        .bind(status_kind)
        .bind(status_msg)
        .bind(&snapshot_enc)
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
        // WHY: the guard blocks overwriting a terminal row with a DIFFERENT
        // terminal status (e.g. cancel wrote `Cancelled`, runner tries
        // `Completed`). But it ALLOWS idempotent same-status writes
        // (`Cancelled` → `Cancelled`) so the runner can persist its collected
        // results onto a row that cancel already flipped to `Cancelled` (W-F).
        let status_char = status.as_db_char();
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
