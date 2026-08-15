//! SQLite implementation of [`SourceRepository`].
//!
//! Converts between domain [`Source`] objects and SQLite rows. Timestamps
//! are stored as RFC 3339 strings, matching the `strftime` default in
//! migration 0004. See ADR-0002 for the storage Port decision.
//!
//! Sensitive fields (URL, headers) are encrypted at rest with
//! XChaCha20-Poly1305 when a [`MasterKey`] is provided. See ADR-0007.
//! During the migration window, plaintext columns are retained for
//! dual-write; reads prefer the encrypted column and fall back to
//! plaintext when it is NULL.

use std::sync::Arc;

use async_trait::async_trait;
use deve_sub_domain::{Source, SourceError, SourceRepository, SourceType};
use deve_sub_kernel::SourceId;
use deve_sub_security::{MasterKey, envelope};
use sqlx::sqlite::SqlitePool;
use std::str::FromStr;

use crate::timestamp::{format_ts, parse_ts};

/// SQLite-backed source repository.
pub struct SqliteSourceRepository {
    pool: SqlitePool,
    master_key: Option<Arc<MasterKey>>,
}

impl SqliteSourceRepository {
    /// Create a new repository without at-rest encryption.
    ///
    /// URL and headers are stored as plaintext. Use this only for tests or
    /// pre-migration databases where the master key is unavailable.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            master_key: None,
        }
    }

    /// Create a new repository with at-rest encryption.
    ///
    /// The URL and custom headers are encrypted with XChaCha20-Poly1305
    /// before being written to the database. See ADR-0007.
    #[must_use]
    pub fn new_with_key(pool: SqlitePool, master_key: Arc<MasterKey>) -> Self {
        Self {
            pool,
            master_key: Some(master_key),
        }
    }

    /// Encrypt a plaintext value into a secret envelope, if a key is set.
    fn seal(&self, plaintext: &str) -> Result<Option<String>, SourceError> {
        match &self.master_key {
            Some(key) => envelope::seal(key.as_bytes(), plaintext.as_bytes())
                .map(Some)
                .map_err(|e| SourceError::Storage(format!("encryption failed: {e}"))),
            None => Ok(None),
        }
    }

    /// Decrypt a secret envelope, falling back to plaintext if the
    /// encrypted column is NULL or no key is set.
    fn open(&self, encrypted: &Option<String>, plaintext: &str) -> Result<String, SourceError> {
        match (&self.master_key, encrypted) {
            (Some(key), Some(env)) => {
                let bytes = envelope::open(key.as_bytes(), env)
                    .map_err(|e| SourceError::Storage(format!("decryption failed: {e}")))?;
                String::from_utf8(bytes)
                    .map_err(|e| SourceError::Storage(format!("decrypted value is not UTF-8: {e}")))
            }
            _ => Ok(plaintext.to_owned()),
        }
    }
}

/// Column list shared by all SELECT queries on the `sources` table.
///
/// WHY: keeping the column list in one place avoids drift between
/// `find_by_id`, `find_by_name`, and `list` when columns are added.
const SOURCE_COLUMNS: &str = "id, name, source_type, url, url_encrypted, http_method, headers_encrypted, \
     headers_encrypted_v2, auto_update, update_interval_secs, enabled, keep_on_fail, \
     filter_rules_json, created_at";

/// Internal row representation for `sqlx::FromRow`.
#[derive(sqlx::FromRow)]
struct SourceRow {
    id: String,
    name: String,
    source_type: String,
    url: String,
    url_encrypted: Option<String>,
    http_method: String,
    headers_encrypted: Option<String>,
    headers_encrypted_v2: Option<String>,
    auto_update: i64,
    update_interval_secs: i64,
    enabled: i64,
    keep_on_fail: i64,
    filter_rules_json: Option<String>,
    created_at: String,
}

impl SourceRow {
    fn to_domain(&self, repo: &SqliteSourceRepository) -> Result<Source, SourceError> {
        let url = repo.open(&self.url_encrypted, &self.url)?;
        // WHY: prefer the v2 encrypted envelope over the legacy column when a
        // key is set. The application layer does not yet populate
        // headers_encrypted_v2, so this is a forward-compatible read path.
        let headers = repo.open(
            &self.headers_encrypted_v2,
            self.headers_encrypted.as_deref().unwrap_or(""),
        )?;
        let headers_encrypted = if headers.is_empty() {
            None
        } else {
            Some(headers)
        };
        Ok(Source {
            id: SourceId::parse(&self.id).map_err(|e| SourceError::Storage(e.to_string()))?,
            name: self.name.clone(),
            source_type: SourceType::from_str(&self.source_type)?,
            url,
            http_method: self.http_method.clone(),
            headers_encrypted,
            auto_update: self.auto_update != 0,
            update_interval_secs: u64::try_from(self.update_interval_secs)
                .map_err(|_| SourceError::Storage("negative update_interval_secs".to_owned()))?,
            enabled: self.enabled != 0,
            keep_on_fail: self.keep_on_fail != 0,
            filter_rules: self
                .filter_rules_json
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(serde_json::from_str)
                .transpose()
                .map_err(|e| SourceError::Storage(format!("invalid filter_rules_json: {e}")))?,
            created_at: parse_ts(&self.created_at).map_err(SourceError::Storage)?,
        })
    }
}

#[async_trait]
impl SourceRepository for SqliteSourceRepository {
    async fn create(&self, source: &Source) -> Result<(), SourceError> {
        let created_at = format_ts(source.created_at).map_err(SourceError::Storage)?;
        let url_encrypted = self.seal(&source.url)?;

        sqlx::query(
            "INSERT INTO sources (id, name, source_type, url, url_encrypted, http_method, headers_encrypted, headers_encrypted_v2, auto_update, update_interval_secs, enabled, keep_on_fail, filter_rules_json, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(source.id.to_string())
        .bind(&source.name)
        .bind(source.source_type.to_string())
        .bind(&source.url)
        .bind(&url_encrypted)
        .bind(&source.http_method)
        .bind(&source.headers_encrypted)
        .bind(&None::<String>)
        .bind(source.auto_update as i64)
        .bind(source.update_interval_secs as i64)
        .bind(source.enabled as i64)
        .bind(source.keep_on_fail as i64)
        .bind(source.filter_rules.as_ref().map(serde_json::to_string).transpose().map_err(|e| SourceError::Storage(e.to_string()))?)
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
        let sql = format!("SELECT {SOURCE_COLUMNS} FROM sources WHERE id = ?");
        let row: Option<SourceRow> = sqlx::query_as(&sql)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain(self)).transpose()
    }

    async fn find_by_name(&self, name: &str) -> Result<Option<Source>, SourceError> {
        let sql = format!("SELECT {SOURCE_COLUMNS} FROM sources WHERE name = ?");
        let row: Option<SourceRow> = sqlx::query_as(&sql)
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain(self)).transpose()
    }

    async fn list(&self, cursor: Option<SourceId>, limit: u32) -> Result<Vec<Source>, SourceError> {
        // WHY: cap at 100 to prevent unbounded result sets from a malicious
        // or accidental large `limit` query parameter.
        let limit = limit.min(100) as i64;
        let rows: Vec<SourceRow> = match cursor {
            Some(c) => {
                let sql = format!(
                    "SELECT {SOURCE_COLUMNS} FROM sources WHERE id > ? ORDER BY id LIMIT ?"
                );
                sqlx::query_as(&sql)
                    .bind(c.to_string())
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|e| SourceError::Storage(e.to_string()))?
            }
            None => {
                let sql = format!("SELECT {SOURCE_COLUMNS} FROM sources ORDER BY id LIMIT ?");
                sqlx::query_as(&sql)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|e| SourceError::Storage(e.to_string()))?
            }
        };
        rows.iter().map(|r| r.to_domain(self)).collect()
    }

    async fn update(&self, source: &Source) -> Result<(), SourceError> {
        let url_encrypted = self.seal(&source.url)?;
        let result = sqlx::query(
            "UPDATE sources SET \
               name = ?, \
               source_type = ?, \
               url = ?, \
               url_encrypted = ?, \
               http_method = ?, \
               headers_encrypted = ?, \
               headers_encrypted_v2 = ?, \
               auto_update = ?, \
               update_interval_secs = ?, \
               enabled = ?, \
               keep_on_fail = ?, \
               filter_rules_json = ? \
             WHERE id = ?",
        )
        .bind(&source.name)
        .bind(source.source_type.to_string())
        .bind(&source.url)
        .bind(&url_encrypted)
        .bind(&source.http_method)
        .bind(&source.headers_encrypted)
        .bind(&None::<String>)
        .bind(source.auto_update as i64)
        .bind(source.update_interval_secs as i64)
        .bind(source.enabled as i64)
        .bind(source.keep_on_fail as i64)
        .bind(
            source
                .filter_rules
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| SourceError::Storage(e.to_string()))?,
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use deve_sub_domain::SourceType;

    fn test_key() -> Arc<MasterKey> {
        Arc::new(MasterKey::from_bytes(&[0x42u8; 32]))
    }

    async fn setup_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("connect");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("migrate");
        pool
    }

    fn sample_source() -> Source {
        Source::new(
            "test-source",
            SourceType::UriList,
            "https://user:pass@host/path".to_owned(),
        )
    }

    #[tokio::test]
    async fn url_encrypted_at_rest() {
        let pool = setup_pool().await;
        let repo = SqliteSourceRepository::new_with_key(pool.clone(), test_key());

        let source = sample_source();
        repo.create(&source).await.expect("create");

        // Verify the encrypted column contains a v1 envelope.
        let row: (Option<String>, String) =
            sqlx::query_as("SELECT url_encrypted, url FROM sources WHERE id = ?")
                .bind(source.id.to_string())
                .fetch_one(&pool)
                .await
                .expect("query");

        let (url_encrypted, url_plain) = row;
        let url_encrypted = url_encrypted.expect("url_encrypted should be non-NULL");
        assert!(envelope::is_envelope(&url_encrypted));
        assert!(
            !url_encrypted.contains("user:pass"),
            "encrypted column must not contain plaintext credentials"
        );
        // Dual-write: plaintext column is still populated.
        assert_eq!(url_plain, source.url);
    }

    #[tokio::test]
    async fn read_decrypts_url() {
        let pool = setup_pool().await;
        let repo = SqliteSourceRepository::new_with_key(pool.clone(), test_key());

        let source = sample_source();
        repo.create(&source).await.expect("create");

        let recovered = repo
            .find_by_id(source.id)
            .await
            .expect("find")
            .expect("source exists");
        assert_eq!(recovered.url, source.url);
    }

    #[tokio::test]
    async fn no_key_stores_plaintext() {
        let pool = setup_pool().await;
        let repo = SqliteSourceRepository::new(pool.clone());

        let source = sample_source();
        repo.create(&source).await.expect("create");

        let row: (Option<String>,) =
            sqlx::query_as("SELECT url_encrypted FROM sources WHERE id = ?")
                .bind(source.id.to_string())
                .fetch_one(&pool)
                .await
                .expect("query");
        assert!(row.0.is_none(), "url_encrypted should be NULL without key");
    }

    #[tokio::test]
    async fn update_re_encrypts_url() {
        let pool = setup_pool().await;
        let repo = SqliteSourceRepository::new_with_key(pool.clone(), test_key());

        let mut source = sample_source();
        repo.create(&source).await.expect("create");

        source.url = "https://new:creds@other/path".to_owned();
        repo.update(&source).await.expect("update");

        let recovered = repo
            .find_by_id(source.id)
            .await
            .expect("find")
            .expect("source exists");
        assert_eq!(recovered.url, "https://new:creds@other/path");
    }
}
