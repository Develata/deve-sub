//! SQLite implementation of [`SourceRepository`].
//!
//! Converts between domain [`Source`] objects and SQLite rows. Timestamps
//! are stored as RFC 3339 strings, matching the `strftime` default in
//! migration 0004. See ADR-0002 for the storage Port decision.

use async_trait::async_trait;
use deve_sub_domain::{Source, SourceError, SourceRepository, SourceType};
use deve_sub_kernel::SourceId;
use sqlx::sqlite::SqlitePool;
use std::str::FromStr;

use crate::timestamp::{format_ts, parse_ts};

/// SQLite-backed source repository.
pub struct SqliteSourceRepository {
    pool: SqlitePool,
}

impl SqliteSourceRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Internal row representation for `sqlx::FromRow`.
#[derive(sqlx::FromRow)]
struct SourceRow {
    id: String,
    name: String,
    source_type: String,
    url: String,
    http_method: String,
    headers_encrypted: Option<String>,
    auto_update: i64,
    update_interval_secs: i64,
    enabled: i64,
    keep_on_fail: i64,
    created_at: String,
}

impl SourceRow {
    fn to_domain(&self) -> Result<Source, SourceError> {
        Ok(Source {
            id: SourceId::parse(&self.id).map_err(|e| SourceError::Storage(e.to_string()))?,
            name: self.name.clone(),
            source_type: SourceType::from_str(&self.source_type)?,
            url: self.url.clone(),
            http_method: self.http_method.clone(),
            headers_encrypted: self.headers_encrypted.clone(),
            auto_update: self.auto_update != 0,
            update_interval_secs: u64::try_from(self.update_interval_secs)
                .map_err(|_| SourceError::Storage("negative update_interval_secs".to_owned()))?,
            enabled: self.enabled != 0,
            keep_on_fail: self.keep_on_fail != 0,
            created_at: parse_ts(&self.created_at).map_err(SourceError::Storage)?,
        })
    }
}

#[async_trait]
impl SourceRepository for SqliteSourceRepository {
    async fn create(&self, source: &Source) -> Result<(), SourceError> {
        let created_at = format_ts(source.created_at).map_err(SourceError::Storage)?;

        sqlx::query(
            "INSERT INTO sources (id, name, source_type, url, http_method, headers_encrypted, auto_update, update_interval_secs, enabled, keep_on_fail, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(source.id.to_string())
        .bind(&source.name)
        .bind(source.source_type.to_string())
        .bind(&source.url)
        .bind(&source.http_method)
        .bind(&source.headers_encrypted)
        .bind(source.auto_update as i64)
        .bind(source.update_interval_secs as i64)
        .bind(source.enabled as i64)
        .bind(source.keep_on_fail as i64)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                SourceError::NameExists
            } else {
                SourceError::Storage(msg)
            }
        })?;
        Ok(())
    }

    async fn find_by_id(&self, id: SourceId) -> Result<Option<Source>, SourceError> {
        let row: Option<SourceRow> = sqlx::query_as(
            "SELECT id, name, source_type, url, http_method, headers_encrypted, auto_update, update_interval_secs, enabled, keep_on_fail, created_at \
             FROM sources WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn find_by_name(&self, name: &str) -> Result<Option<Source>, SourceError> {
        let row: Option<SourceRow> = sqlx::query_as(
            "SELECT id, name, source_type, url, http_method, headers_encrypted, auto_update, update_interval_secs, enabled, keep_on_fail, created_at \
             FROM sources WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn list(&self, cursor: Option<SourceId>, limit: u32) -> Result<Vec<Source>, SourceError> {
        // WHY: cap at 100 to prevent unbounded result sets from a malicious
        // or accidental large `limit` query parameter.
        let limit = limit.min(100) as i64;
        let rows: Vec<SourceRow> = match cursor {
            Some(c) => {
                sqlx::query_as(
                    "SELECT id, name, source_type, url, http_method, headers_encrypted, auto_update, update_interval_secs, enabled, keep_on_fail, created_at \
                     FROM sources WHERE id > ? ORDER BY id LIMIT ?",
                )
                .bind(c.to_string())
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| SourceError::Storage(e.to_string()))?
            }
            None => {
                sqlx::query_as(
                    "SELECT id, name, source_type, url, http_method, headers_encrypted, auto_update, update_interval_secs, enabled, keep_on_fail, created_at \
                     FROM sources ORDER BY id LIMIT ?",
                )
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| SourceError::Storage(e.to_string()))?
            }
        };
        rows.iter().map(|r| r.to_domain()).collect()
    }

    async fn update(&self, source: &Source) -> Result<(), SourceError> {
        let result = sqlx::query(
            "UPDATE sources SET \
               name = ?, \
               source_type = ?, \
               url = ?, \
               http_method = ?, \
               headers_encrypted = ?, \
               auto_update = ?, \
               update_interval_secs = ?, \
               enabled = ?, \
               keep_on_fail = ? \
             WHERE id = ?",
        )
        .bind(&source.name)
        .bind(source.source_type.to_string())
        .bind(&source.url)
        .bind(&source.http_method)
        .bind(&source.headers_encrypted)
        .bind(source.auto_update as i64)
        .bind(source.update_interval_secs as i64)
        .bind(source.enabled as i64)
        .bind(source.keep_on_fail as i64)
        .bind(source.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                SourceError::NameExists
            } else {
                SourceError::Storage(msg)
            }
        })?;
        if result.rows_affected() == 0 {
            return Err(SourceError::SourceNotFound);
        }
        Ok(())
    }

    async fn delete(&self, id: SourceId) -> Result<(), SourceError> {
        // WHY: ON DELETE CASCADE in migration 0004 removes snapshots, items,
        // and node_source_bindings automatically. No manual cascade needed.
        let result = sqlx::query("DELETE FROM sources WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SourceError::SourceNotFound);
        }
        Ok(())
    }
}
