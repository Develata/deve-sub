//! Configuration edits retain omitted secrets in the current storage snapshot.

use super::*;

impl SqliteSourceRepository {
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
        crate::pool_meta_repository::bump_revision_tx(&mut tx).await?;
        tx.commit()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(source)
    }
}
