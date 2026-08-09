//! SQLite implementation of [`TemplateVersionRepository`].
//!
//! Creating a new version deactivates the previous active version for the
//! same template in a single transaction, so there is never a window where
//! a template has zero or two active versions. Rollback re-activates a prior
//! version in the same atomic fashion. See ADR-0002.

use async_trait::async_trait;
use deve_sub_domain::{TemplateError, TemplateSpec, TemplateVersion, TemplateVersionRepository};
use deve_sub_kernel::{TemplateId, TemplateVersionId};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

/// SQLite-backed template version repository.
pub struct SqliteTemplateVersionRepository {
    pool: SqlitePool,
}

impl SqliteTemplateVersionRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Internal row representation for `sqlx::FromRow`.
#[derive(sqlx::FromRow)]
struct VersionRow {
    id: String,
    template_id: String,
    version: i64,
    spec_json: String,
    spec_yaml: String,
    is_active: i64,
    created_at: String,
}

impl VersionRow {
    fn to_domain(&self) -> Result<TemplateVersion, TemplateError> {
        let spec: TemplateSpec = serde_json::from_str(&self.spec_json)
            .map_err(|e| TemplateError::Storage(format!("invalid spec_json: {e}")))?;
        Ok(TemplateVersion {
            id: TemplateVersionId::parse(&self.id)
                .map_err(|e| TemplateError::Storage(e.to_string()))?,
            template_id: TemplateId::parse(&self.template_id)
                .map_err(|e| TemplateError::Storage(e.to_string()))?,
            version: u64::try_from(self.version)
                .map_err(|_| TemplateError::Storage("negative version".to_owned()))?,
            spec,
            spec_yaml: self.spec_yaml.clone(),
            is_active: self.is_active != 0,
            created_at: parse_ts(&self.created_at).map_err(TemplateError::Storage)?,
        })
    }
}

#[async_trait]
impl TemplateVersionRepository for SqliteTemplateVersionRepository {
    async fn create(&self, version: &TemplateVersion) -> Result<(), TemplateError> {
        // WHY: deactivate-then-insert in a single transaction so there is
        // never a window where a template has two active versions or zero
        // active versions between the deactivation and the insert.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;

        sqlx::query(
            "UPDATE template_versions SET is_active = 0 WHERE template_id = ? AND is_active = 1",
        )
        .bind(version.template_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;

        let spec_json = serde_json::to_string(&version.spec)
            .map_err(|e| TemplateError::Storage(format!("spec serialize: {e}")))?;
        let created_at = format_ts(version.created_at).map_err(TemplateError::Storage)?;

        sqlx::query(
            "INSERT INTO template_versions (id, template_id, version, spec_json, spec_yaml, is_active, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(version.id.to_string())
        .bind(version.template_id.to_string())
        .bind(version.version as i64)
        .bind(&spec_json)
        .bind(&version.spec_yaml)
        .bind(version.is_active as i64)
        .bind(created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_active(
        &self,
        template_id: TemplateId,
    ) -> Result<Option<TemplateVersion>, TemplateError> {
        let row: Option<VersionRow> = sqlx::query_as(
            "SELECT id, template_id, version, spec_json, spec_yaml, is_active, created_at \
             FROM template_versions \
             WHERE template_id = ? AND is_active = 1",
        )
        .bind(template_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn find_by_id(
        &self,
        id: TemplateVersionId,
    ) -> Result<Option<TemplateVersion>, TemplateError> {
        let row: Option<VersionRow> = sqlx::query_as(
            "SELECT id, template_id, version, spec_json, spec_yaml, is_active, created_at \
             FROM template_versions \
             WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn find_by_version_number(
        &self,
        template_id: TemplateId,
        version: u64,
    ) -> Result<Option<TemplateVersion>, TemplateError> {
        let row: Option<VersionRow> = sqlx::query_as(
            "SELECT id, template_id, version, spec_json, spec_yaml, is_active, created_at \
             FROM template_versions \
             WHERE template_id = ? AND version = ?",
        )
        .bind(template_id.to_string())
        .bind(version as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn list_for_template(
        &self,
        template_id: TemplateId,
        limit: u32,
    ) -> Result<Vec<TemplateVersion>, TemplateError> {
        let limit = limit.min(100) as i64;
        let rows: Vec<VersionRow> = sqlx::query_as(
            "SELECT id, template_id, version, spec_json, spec_yaml, is_active, created_at \
             FROM template_versions \
             WHERE template_id = ? \
             ORDER BY version DESC \
             LIMIT ?",
        )
        .bind(template_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;
        rows.iter().map(|r| r.to_domain()).collect()
    }

    async fn activate(
        &self,
        version_id: TemplateVersionId,
    ) -> Result<TemplateVersion, TemplateError> {
        // WHY: atomic activate-by-id. Load the version first to get its
        // template_id, then deactivate all siblings and activate the target
        // in a single transaction. The partial unique index
        // idx_template_versions_single_active guarantees the invariant at
        // the DB level as defense-in-depth.
        let version = self
            .find_by_id(version_id)
            .await?
            .ok_or(TemplateError::VersionNotFound)?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;

        sqlx::query(
            "UPDATE template_versions SET is_active = 0 WHERE template_id = ? AND is_active = 1",
        )
        .bind(version.template_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;

        sqlx::query("UPDATE template_versions SET is_active = 1 WHERE id = ?")
            .bind(version_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;

        // WHY: also update the aggregate's active_version_id/active_version
        // so reads through TemplateRepository reflect the rollback without a
        // separate join. This keeps the two tables consistent within the
        // same transaction.
        let updated_at =
            format_ts(deve_sub_kernel::Timestamp::now()).map_err(TemplateError::Storage)?;
        sqlx::query(
            "UPDATE templates SET \
               active_version_id = ?, \
               active_version = ?, \
               updated_at = ? \
             WHERE id = ?",
        )
        .bind(version_id.to_string())
        .bind(version.version as i64)
        .bind(updated_at)
        .bind(version.template_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| TemplateError::Storage(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| TemplateError::Storage(e.to_string()))?;

        let mut activated = version;
        activated.is_active = true;
        Ok(activated)
    }
}
