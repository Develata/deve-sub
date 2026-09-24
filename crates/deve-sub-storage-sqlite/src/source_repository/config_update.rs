//! Source configuration writes preserve current secrets and respect refresh leases.

use super::*;

impl SqliteSourceRepository {
    pub(super) async fn apply_failure_policy(
        &self,
        id: SourceId,
        job_id: SourceRefreshJobId,
    ) -> Result<(), SourceError> {
        // WHY: a failed old fetch may return after lease reclamation and an
        // administrator edit; only its still-live lease can apply failure policy.
        sqlx::query("UPDATE sources SET enabled = 0 WHERE id = ? AND keep_on_fail = 0 AND EXISTS (SELECT 1 FROM source_refresh_jobs WHERE id = ? AND source_id = sources.id AND status = 'R')")
            .bind(id.to_string())
            .bind(job_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }

    pub(super) async fn apply_config(
        &self,
        update: &SourceConfigUpdate,
    ) -> Result<Source, SourceError> {
        let url_encrypted = update
            .url
            .as_deref()
            .map(|url| self.seal(CTX_URL, url))
            .transpose()?
            .flatten();
        let filters = update
            .filter_rules
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        guard_config_edit(&mut tx, update.id).await?;
        // WHY: the UPDATE obtains write admission before reading current fields.
        // Omitted URL, HTTP method and headers stay in the row; an earlier
        // application read cannot restore stale secrets over another edit.
        let sql = format!(
            "UPDATE sources SET name = ?, source_type = ?, \
             url_encrypted = COALESCE(?, url_encrypted), auto_update = ?, \
             update_interval_secs = ?, enabled = ?, keep_on_fail = ?, filter_rules_json = ? \
             WHERE id = ? RETURNING {SOURCE_COLUMNS}"
        );
        let row: SourceRow = sqlx::query_as(&sql)
            .bind(&update.name)
            .bind(update.source_type.to_string())
            .bind(url_encrypted)
            .bind(i64::from(update.auto_update))
            .bind(update.update_interval_secs as i64)
            .bind(i64::from(update.enabled))
            .bind(i64::from(update.keep_on_fail))
            .bind(filters)
            .bind(update.id.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| {
                if crate::error_classify::is_unique_violation(&e) {
                    SourceError::NameExists
                } else {
                    SourceError::Storage(e.to_string())
                }
            })?
            .ok_or(SourceError::SourceNotFound)?;
        let source = row.to_domain(self)?;
        invalidate_fetch_validator(&mut tx, update.id).await?;
        crate::pool_meta_repository::bump_revision_tx(&mut tx).await?;
        tx.commit()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(source)
    }
}

/// Serialize administrator edits with the refresh lease before reading it.
pub(super) async fn guard_config_edit(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: SourceId,
) -> Result<(), SourceError> {
    // WHY: a deferred read transaction can race with a new refresh lease.
    // A no-op write takes admission first, without rewriting stale secrets.
    sqlx::query("UPDATE sources SET id = id WHERE id = ?")
        .bind(id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
    let running: Option<String> = sqlx::query_scalar(
        "SELECT id FROM source_refresh_jobs WHERE source_id = ? AND status = 'R'",
    )
    .bind(id.to_string())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| SourceError::Storage(e.to_string()))?;
    if running.is_some() {
        return Err(SourceError::RefreshInProgress(id.to_string()));
    }
    Ok(())
}

/// Configuration edits require a fresh body before filtering and parsing.
pub(super) async fn invalidate_fetch_validator(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: SourceId,
) -> Result<(), SourceError> {
    sqlx::query("UPDATE source_snapshots SET etag = NULL WHERE source_id = ? AND is_active = 1")
        .bind(id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
    Ok(())
}
