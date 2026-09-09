//! Probe source persistence and atomic counter/delta commit.
use crate::{
    discriminant::SqliteDiscriminant,
    timestamp::{format_ts, parse_ts},
};
use async_trait::async_trait;
use deve_sub_domain::{ProbeError, ProbeSource, ProbeSourceKind, ProbeSourceRepository};
use deve_sub_kernel::ProbeSourceId;
use deve_sub_security::{MasterKey, envelope};
use sqlx::SqlitePool;
use std::sync::Arc;
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

    async fn update_on(
        &self,
        source: &ProbeSource,
        connection: &mut sqlx::SqliteConnection,
    ) -> Result<(), ProbeError> {
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
             last_sync_status = ?, last_counter_snapshot = ?, updated_at = ?, revision = revision + 1 WHERE id = ? AND revision = ?",
        )
        .bind(source.kind.encode())
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
        .bind(i64::try_from(source.revision).map_err(|_| ProbeError::Conflict)?)
        .execute(connection)
        .await
        .map_err(|e| {
            if crate::error_classify::is_unique_violation(&e) {
                ProbeError::NameExists
            } else {
                ProbeError::Storage(e.to_string())
            }
        })?;
        if result.rows_affected() == 0 {
            return Err(ProbeError::Conflict);
        }
        Ok(())
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
    revision: i64,
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
        let kind = ProbeSourceKind::decode(&self.kind)
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
            revision: self.revision as u64,
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
        .bind(source.kind.encode())
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
            if crate::error_classify::is_unique_violation(&e) {
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
             created_at, updated_at, revision FROM probe_sources WHERE id = ?",
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
        // WHY: self-cap at 100 like all sibling list methods (SRC-020).
        let limit = limit.min(100);
        let kind_char = kind.map(|k| k.encode().to_owned());
        let rows: Vec<ProbeSourceRow> = if let Some(c) = cursor {
            if let Some(k) = kind_char {
                sqlx::query_as(
                    "SELECT id, kind, name, endpoint_url, auth_config, subscription_id, enabled, \
                     last_sync_at, last_sync_status_kind, last_sync_status, last_counter_snapshot, \
                     created_at, updated_at, revision FROM probe_sources \
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
                     created_at, updated_at, revision FROM probe_sources \
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
                 created_at, updated_at, revision FROM probe_sources \
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
                 created_at, updated_at, revision FROM probe_sources \
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
        let mut connection = self
            .pool
            .acquire()
            .await
            .map_err(|e| ProbeError::Storage(e.to_string()))?;
        self.update_on(source, &mut connection).await
    }

    async fn commit_sync(
        &self,
        source: &ProbeSource,
        records: &[deve_sub_domain::TrafficRecord],
    ) -> Result<(), ProbeError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| ProbeError::Storage(e.to_string()))?;
        self.update_on(source, &mut tx).await?;
        for record in records {
            if Some(record.subscription_id) != source.subscription_id
                || record.source_kind != deve_sub_domain::TrafficSourceKind::Probe
            {
                return Err(ProbeError::InvalidInput(
                    "traffic does not belong to probe source".into(),
                ));
            }
            sqlx::query("INSERT INTO subscription_traffic (id, subscription_id, source_kind, upload, download, recorded_at, source_ref) VALUES (?, ?, 'P', ?, ?, ?, ?)")
                .bind(record.id.to_string()).bind(record.subscription_id.to_string())
                .bind(i64::try_from(record.upload).map_err(|_| ProbeError::InvalidInput("traffic overflow".into()))?)
                .bind(i64::try_from(record.download).map_err(|_| ProbeError::InvalidInput("traffic overflow".into()))?)
                .bind(format_ts(record.recorded_at).map_err(ProbeError::Storage)?)
                .bind(&record.source_ref).execute(&mut *tx).await
                .map_err(|e| ProbeError::Storage(e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| ProbeError::Storage(e.to_string()))
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
