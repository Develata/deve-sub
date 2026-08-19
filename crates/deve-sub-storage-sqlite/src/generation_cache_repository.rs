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
use sqlx::sqlite::SqlitePool;

pub struct SqliteGenerationCacheRepository {
    pool: SqlitePool,
}

impl SqliteGenerationCacheRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
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
    content: String,
    is_active: i64,
}

impl CacheRow {
    fn to_domain(&self) -> Result<GenerationCacheEntry, TemplateError> {
        Ok(GenerationCacheEntry {
            id: GenerationCacheId::parse(&self.id)
                .map_err(|e| TemplateError::Storage(e.to_string()))?,
            template_id: TemplateId::parse(&self.template_id)
                .map_err(|e| TemplateError::Storage(e.to_string()))?,
            template_version: u64::try_from(self.template_version)
                .map_err(|_| TemplateError::Storage("negative template_version".to_owned()))?,
            profile: self.profile.clone(),
            mode: self.mode.clone(),
            selection_mode: self.selection_mode.clone(),
            selection_payload: self.selection_payload.clone(),
            pool_revision: u64::try_from(self.pool_revision)
                .map_err(|_| TemplateError::Storage("negative pool_revision".to_owned()))?,
            cache_key: self.cache_key.clone(),
            content: self.content.clone(),
            is_active: self.is_active != 0,
        })
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
             selection_payload, pool_revision, cache_key, content, is_active \
             FROM generation_cache WHERE cache_key = ?",
        )
        .bind(cache_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn find_active(
        &self,
        template_id: TemplateId,
        profile: &str,
    ) -> Result<Option<GenerationCacheEntry>, TemplateError> {
        let row: Option<CacheRow> = sqlx::query_as(
            "SELECT id, template_id, template_version, profile, mode, selection_mode, \
             selection_payload, pool_revision, cache_key, content, is_active \
             FROM generation_cache WHERE template_id = ? AND profile = ? AND is_active = 1",
        )
        .bind(template_id.to_string())
        .bind(profile)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn store(&self, entry: &GenerationCacheEntry) -> Result<(), TemplateError> {
        sqlx::query(
            "INSERT INTO generation_cache \
             (id, template_id, template_version, profile, mode, selection_mode, \
              selection_payload, pool_revision, cache_key, content, is_active) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .bind(&entry.content)
        .bind(i64::from(entry.is_active))
        .execute(&self.pool)
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
            .begin()
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;

        sqlx::query(
            "UPDATE generation_cache SET is_active = 0 \
             WHERE template_id = ? AND profile = ? AND is_active = 1",
        )
        .bind(template_id.to_string())
        .bind(profile)
        .execute(&mut *tx)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;

        let result = sqlx::query("UPDATE generation_cache SET is_active = 1 WHERE id = ?")
            .bind(new_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(TemplateError::Storage(
                "cache entry to activate not found".to_owned(),
            ));
        }

        tx.commit()
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;
        Ok(())
    }
}
