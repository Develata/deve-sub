//! SQLite implementation of [`TemplateRepository`].
//!
//! Converts between domain [`SubscriptionTemplate`] objects and SQLite rows.
//! Timestamps are stored as RFC 3339 strings, matching the `strftime` default
//! in migration 0007. See ADR-0002 for the storage Port decision.

use async_trait::async_trait;
use deve_sub_domain::{SubscriptionTemplate, TemplateError, TemplateRepository};
use deve_sub_kernel::TemplateId;
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

/// SQLite-backed template repository.
pub struct SqliteTemplateRepository {
    pool: SqlitePool,
}

impl SqliteTemplateRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Internal row representation for `sqlx::FromRow`.
#[derive(sqlx::FromRow)]
struct TemplateRow {
    id: String,
    name: String,
    description: String,
    active_version_id: Option<String>,
    active_version: i64,
    created_at: String,
    updated_at: String,
}

impl TemplateRow {
    fn to_domain(&self) -> Result<SubscriptionTemplate, TemplateError> {
        let active_version_id = self
            .active_version_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(deve_sub_kernel::TemplateVersionId::parse)
            .transpose()
            .map_err(|e| TemplateError::Storage(e.to_string()))?;
        Ok(SubscriptionTemplate {
            id: TemplateId::parse(&self.id).map_err(|e| TemplateError::Storage(e.to_string()))?,
            name: self.name.clone(),
            description: self.description.clone(),
            active_version_id,
            active_version: u64::try_from(self.active_version)
                .map_err(|_| TemplateError::Storage("negative active_version".to_owned()))?,
            created_at: parse_ts(&self.created_at).map_err(TemplateError::Storage)?,
            updated_at: parse_ts(&self.updated_at).map_err(TemplateError::Storage)?,
        })
    }
}

#[async_trait]
impl TemplateRepository for SqliteTemplateRepository {
    async fn create(&self, template: &SubscriptionTemplate) -> Result<(), TemplateError> {
        let created_at = format_ts(template.created_at).map_err(TemplateError::Storage)?;
        let updated_at = format_ts(template.updated_at).map_err(TemplateError::Storage)?;
        let active_version_id = template
            .active_version_id
            .map(|id| id.to_string())
            .unwrap_or_default();

        sqlx::query(
            "INSERT INTO templates (id, name, description, active_version_id, active_version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(template.id.to_string())
        .bind(&template.name)
        .bind(&template.description)
        .bind(&active_version_id)
        .bind(template.active_version as i64)
        .bind(created_at)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                TemplateError::NameExists
            } else {
                TemplateError::Storage(msg)
            }
        })?;
        Ok(())
    }

    async fn find_by_id(
        &self,
        id: TemplateId,
    ) -> Result<Option<SubscriptionTemplate>, TemplateError> {
        let row: Option<TemplateRow> = sqlx::query_as(
            "SELECT id, name, description, active_version_id, active_version, created_at, updated_at \
             FROM templates WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn find_by_name(
        &self,
        name: &str,
    ) -> Result<Option<SubscriptionTemplate>, TemplateError> {
        let row: Option<TemplateRow> = sqlx::query_as(
            "SELECT id, name, description, active_version_id, active_version, created_at, updated_at \
             FROM templates WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn list(
        &self,
        cursor: Option<TemplateId>,
        limit: u32,
    ) -> Result<Vec<SubscriptionTemplate>, TemplateError> {
        let limit = limit.min(100) as i64;
        let rows: Vec<TemplateRow> = match cursor {
            Some(c) => {
                sqlx::query_as(
                    "SELECT id, name, description, active_version_id, active_version, created_at, updated_at \
                     FROM templates WHERE id > ? ORDER BY id LIMIT ?",
                )
                .bind(c.to_string())
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| TemplateError::Storage(e.to_string()))?
            }
            None => {
                sqlx::query_as(
                    "SELECT id, name, description, active_version_id, active_version, created_at, updated_at \
                     FROM templates ORDER BY id LIMIT ?",
                )
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| TemplateError::Storage(e.to_string()))?
            }
        };
        rows.iter().map(|r| r.to_domain()).collect()
    }

    async fn update(&self, template: &SubscriptionTemplate) -> Result<(), TemplateError> {
        let updated_at = format_ts(template.updated_at).map_err(TemplateError::Storage)?;
        let active_version_id = template
            .active_version_id
            .map(|id| id.to_string())
            .unwrap_or_default();

        let result = sqlx::query(
            "UPDATE templates SET \
               name = ?, \
               description = ?, \
               active_version_id = ?, \
               active_version = ?, \
               updated_at = ? \
             WHERE id = ?",
        )
        .bind(&template.name)
        .bind(&template.description)
        .bind(&active_version_id)
        .bind(template.active_version as i64)
        .bind(updated_at)
        .bind(template.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                TemplateError::NameExists
            } else {
                TemplateError::Storage(msg)
            }
        })?;
        if result.rows_affected() == 0 {
            return Err(TemplateError::TemplateNotFound);
        }
        Ok(())
    }

    async fn delete(&self, id: TemplateId) -> Result<(), TemplateError> {
        // WHY: ON DELETE CASCADE in migration 0007 removes template_versions
        // and generation_cache rows automatically. No manual cascade needed.
        let result = sqlx::query("DELETE FROM templates WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(TemplateError::TemplateNotFound);
        }
        Ok(())
    }
}
