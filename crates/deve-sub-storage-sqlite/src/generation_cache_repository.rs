//! SQLite implementation of [`GenerationCacheRepository`].
//!
//! Stores generated subscription content keyed by a SHA-256 cache key. The
//! `activate` method performs atomic publish: deactivate the prior active
//! entry for the same `(template_id, profile)` and activate the new one in a
//! single transaction, enforced by the `idx_generation_cache_single_active`
//! partial unique index (migration 0008, GEN-015, constraint #19).

use async_trait::async_trait;
use deve_sub_domain::{GenerationCacheEntry, GenerationCacheRepository, TemplateError};
use deve_sub_kernel::{GenerationCacheId, TemplateId};
use deve_sub_security::MasterKey;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

mod encryption;
mod migration;
pub use migration::{
    migrate_generation_cache, verify_generation_cache_encryption, verify_generation_cache_payloads,
};

/// Encrypted generated output with atomic publish and per-subscription retention.
pub struct SqliteGenerationCacheRepository {
    pool: SqlitePool,
    key: Arc<MasterKey>,
}

/// Extra inactive entries per template/profile, beyond active and live fallbacks.
const INACTIVE_RETAIN: i64 = 8;

impl SqliteGenerationCacheRepository {
    /// Use the same master key as the other encrypted repositories (ADR-0007).
    #[must_use]
    pub fn new(pool: SqlitePool, key: Arc<MasterKey>) -> Self {
        Self { pool, key }
    }
}

#[derive(sqlx::FromRow)]
struct CacheRow {
    id: String,
    template_id: String,
    template_version: i64,
    profile: String,
    mode: String,
    selection_mode: String,
    selection_payload: String,
    pool_revision: i64,
    cache_key: String,
    content_encrypted: Option<String>,
    content_legacy: String,
    is_active: i64,
}

impl CacheRow {
    fn into_domain(self, key: &MasterKey) -> Result<GenerationCacheEntry, TemplateError> {
        let content = encryption::open(
            key,
            &self.id,
            &self.cache_key,
            self.content_encrypted.as_deref(),
            &self.content_legacy,
        )?;
        let entry = GenerationCacheEntry {
            id: GenerationCacheId::parse(&self.id)
                .map_err(|e| TemplateError::Storage(e.to_string()))?,
            template_id: TemplateId::parse(&self.template_id)
                .map_err(|e| TemplateError::Storage(e.to_string()))?,
            template_version: u64::try_from(self.template_version)
                .map_err(|_| TemplateError::Storage("negative template_version".to_owned()))?,
            profile: self.profile,
            mode: self.mode,
            selection_mode: self.selection_mode,
            selection_payload: self.selection_payload,
            pool_revision: u64::try_from(self.pool_revision)
                .map_err(|_| TemplateError::Storage("negative pool_revision".to_owned()))?,
            cache_key: self.cache_key,
            content,
            is_active: self.is_active != 0,
        };
        if !entry.has_current_semantics() {
            return Err(TemplateError::CacheInvalidated);
        }
        Ok(entry)
    }
}

#[async_trait]
impl GenerationCacheRepository for SqliteGenerationCacheRepository {
    async fn find_by_key(
        &self,
        cache_key: &str,
    ) -> Result<Option<GenerationCacheEntry>, TemplateError> {
        let row: Option<CacheRow> = sqlx::query_as(
            "SELECT id, template_id, template_version, profile, mode, selection_mode, \
             selection_payload, pool_revision, cache_key, content_encrypted, content_legacy, is_active \
             FROM generation_cache WHERE cache_key = ? \
             AND semantics_version = 7 AND pool_revision >= (SELECT withdrawal_revision FROM pool_meta WHERE id = 1)",
        )
        .bind(cache_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        row.map(|row| row.into_domain(&self.key)).transpose()
    }

    async fn find_active(
        &self,
        template_id: TemplateId,
        profile: &str,
    ) -> Result<Option<GenerationCacheEntry>, TemplateError> {
        let row: Option<CacheRow> = sqlx::query_as(
            "SELECT id, template_id, template_version, profile, mode, selection_mode, \
             selection_payload, pool_revision, cache_key, content_encrypted, content_legacy, is_active \
             FROM generation_cache WHERE template_id = ? AND profile = ? AND is_active = 1 \
             AND semantics_version = 7 AND pool_revision >= (SELECT withdrawal_revision FROM pool_meta WHERE id = 1)",
        )
        .bind(template_id.to_string())
        .bind(profile)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        row.map(|row| row.into_domain(&self.key)).transpose()
    }

    async fn find_latest(
        &self,
        template_id: TemplateId,
        profile: &str,
        selection_mode: &str,
        selection_payload: &str,
        version_pin: Option<u64>,
        mode: &str,
    ) -> Result<Option<GenerationCacheEntry>, TemplateError> {
        // WHY: order by id, not pool_revision — ULID ids are monotonic by
        // creation time, so this is the newest stored entry for the
        // selection shape even if revisions were bumped without storing.
        let row: Option<CacheRow> = sqlx::query_as(
            "SELECT id, template_id, template_version, profile, mode, selection_mode, \
             selection_payload, pool_revision, cache_key, content_encrypted, content_legacy, is_active \
             FROM generation_cache \
             WHERE template_id = ? AND profile = ? AND selection_mode = ? \
             AND selection_payload = ? AND (? IS NULL OR template_version = ?) AND mode = ? \
             AND semantics_version = 7 AND pool_revision >= (SELECT withdrawal_revision FROM pool_meta WHERE id = 1) \
             ORDER BY id DESC LIMIT 1",
        )
        .bind(template_id.to_string())
        .bind(profile)
        .bind(selection_mode)
        .bind(selection_payload)
        .bind(version_pin.map(|v| v as i64))
        .bind(version_pin.map(|v| v as i64))
        .bind(mode)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        row.map(|row| row.into_domain(&self.key)).transpose()
    }

    async fn store(&self, entry: &GenerationCacheEntry) -> Result<(), TemplateError> {
        // WHY: never relabel old output merely because its bytes were encrypted.
        if !entry.has_current_semantics() {
            return Err(TemplateError::CacheInvalidated);
        }
        let encrypted = encryption::seal(
            &self.key,
            &entry.id.to_string(),
            &entry.cache_key,
            &entry.content,
        )?;
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;
        migration::bind_key(&mut tx, &self.key)
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;

        // Check the floor in the same write transaction as insertion; a late
        // pre-withdrawal writer must fail without changing key binding or cache.
        let inserted = sqlx::query(
            "INSERT INTO generation_cache \
             (id, template_id, template_version, profile, mode, selection_mode, \
              selection_payload, pool_revision, cache_key, content_encrypted, content_legacy, is_active, semantics_version) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '', ?, 7 \
             WHERE ? >= (SELECT withdrawal_revision FROM pool_meta WHERE id = 1)",
        )
        .bind(entry.id.to_string())
        .bind(entry.template_id.to_string())
        .bind(entry.template_version as i64)
        .bind(&entry.profile)
        .bind(&entry.mode)
        .bind(&entry.selection_mode)
        .bind(&entry.selection_payload)
        .bind(entry.pool_revision as i64)
        .bind(&entry.cache_key)
        .bind(encrypted)
        .bind(i64::from(entry.is_active))
        .bind(entry.pool_revision as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        if inserted.rows_affected() == 0 {
            return Err(TemplateError::CacheInvalidated);
        }

        // WHY (constraint #19): protect each subscription's last-good result,
        // not merely the eight most recent deliveries across all selectors.
        // Both persisted selectors use the same NodeSelector serialization.
        // Materialize protection before deletion; pins and lenient delivery
        // mode must match. Unreferenced history stays bounded even after edits
        // or deletion, and repeated revisions cannot grow this set indefinitely.
        sqlx::query(
            "WITH protected(id) AS MATERIALIZED (\
                 SELECT MAX(c.id) FROM subscriptions s JOIN generation_cache c \
                 ON c.template_id = s.template_id AND c.profile = s.profile \
                 AND c.selection_payload = s.node_selection AND c.mode = 'lenient' \
                 AND (s.template_version_pin IS NULL OR c.template_version = s.template_version_pin) \
                 AND c.semantics_version = 7 AND c.pool_revision >= (SELECT withdrawal_revision FROM pool_meta WHERE id = 1) \
                 WHERE s.template_id = ? AND s.profile = ? GROUP BY s.id) \
             DELETE FROM generation_cache WHERE id IN (\
                 SELECT id FROM generation_cache \
                 WHERE template_id = ? AND profile = ? AND is_active = 0 AND semantics_version = 7 \
                 AND id NOT IN (SELECT id FROM protected) \
                 ORDER BY id DESC LIMIT -1 OFFSET ?)",
        )
        .bind(entry.template_id.to_string())
        .bind(&entry.profile)
        .bind(entry.template_id.to_string())
        .bind(&entry.profile)
        .bind(INACTIVE_RETAIN)
        .execute(&mut *tx)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn activate(
        &self,
        template_id: TemplateId,
        profile: &str,
        new_id: GenerationCacheId,
    ) -> Result<(), TemplateError> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;

        // WHY: authentication failure must not demote the last-good result.
        let row: Option<CacheRow> = sqlx::query_as(
            "SELECT id, template_id, template_version, profile, mode, selection_mode, \
             selection_payload, pool_revision, cache_key, content_encrypted, content_legacy, is_active \
             FROM generation_cache WHERE id = ? AND template_id = ? AND profile = ? \
             AND semantics_version = 7 AND pool_revision >= (SELECT withdrawal_revision FROM pool_meta WHERE id = 1)",
        )
        .bind(new_id.to_string())
        .bind(template_id.to_string())
        .bind(profile)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        row.ok_or(TemplateError::CacheInvalidated)?
            .into_domain(&self.key)?;

        sqlx::query(
            "UPDATE generation_cache SET is_active = 0 \
             WHERE template_id = ? AND profile = ? AND is_active = 1",
        )
        .bind(template_id.to_string())
        .bind(profile)
        .execute(&mut *tx)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;

        let result = sqlx::query("UPDATE generation_cache SET is_active = 1 WHERE id = ? AND template_id = ? AND profile = ? AND semantics_version = 7 AND pool_revision >= (SELECT withdrawal_revision FROM pool_meta WHERE id = 1)")
            .bind(new_id.to_string())
            .bind(template_id.to_string())
            .bind(profile)
            .execute(&mut *tx)
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(TemplateError::CacheInvalidated);
        }

        tx.commit()
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;
        Ok(())
    }
}
