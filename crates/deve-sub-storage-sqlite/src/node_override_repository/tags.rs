//! Atomic tag membership and identity-preserving metadata changes.
use super::*;
use deve_sub_domain::TagUpdateMode;
use std::collections::BTreeSet;

impl SqliteNodeOverrideRepository {
    pub(super) async fn apply_tags(
        &self,
        assignments: &[(NodeId, Vec<TagId>)],
        mode: TagUpdateMode,
    ) -> Result<(), SourceError> {
        if assignments.is_empty() {
            return Ok(());
        }
        // Acquire write admission before reference checks. A concurrent delete
        // cannot invalidate an already-validated tag before membership commit.
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(storage)?;
        for (node, tags) in assignments {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?)")
                    .bind(node.to_string())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(storage)?;
            if !exists {
                return Err(SourceError::NodeNotFound(node.to_string()));
            }
            for tag in tags.iter().collect::<BTreeSet<_>>() {
                let exists: bool =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tags WHERE id = ?)")
                        .bind(tag.to_string())
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(storage)?;
                if !exists {
                    return Err(SourceError::TagNotFound);
                }
            }
        }
        for (node, tags) in assignments {
            if mode == TagUpdateMode::Replace {
                sqlx::query("DELETE FROM node_tags WHERE node_id = ?")
                    .bind(node.to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(storage)?;
            }
            for tag in tags.iter().collect::<BTreeSet<_>>() {
                let sql = if mode == TagUpdateMode::Remove {
                    "DELETE FROM node_tags WHERE node_id = ? AND tag_id = ?"
                } else {
                    "INSERT INTO node_tags (node_id, tag_id) VALUES (?, ?) ON CONFLICT DO NOTHING"
                };
                sqlx::query(sql)
                    .bind(node.to_string())
                    .bind(tag.to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(storage)?;
            }
        }
        crate::pool_meta_repository::bump_revision_tx(&mut tx).await?;
        tx.commit().await.map_err(storage)?;
        Ok(())
    }

    pub(super) async fn rename_tag(
        &self,
        tag_id: TagId,
        name: &str,
        color: Option<&str>,
    ) -> Result<Tag, SourceError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        let changed = sqlx::query("UPDATE tags SET name = ?, color = ? WHERE id = ?")
            .bind(name)
            .bind(color)
            .bind(tag_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                if crate::error_classify::is_unique_violation(&e) {
                    SourceError::TagExists
                } else {
                    storage(e)
                }
            })?;
        if changed.rows_affected() == 0 {
            return Err(SourceError::TagNotFound);
        }
        // Tag-name selectors use the name, so rename invalidates generation.
        crate::pool_meta_repository::bump_revision_tx(&mut tx).await?;
        tx.commit().await.map_err(storage)?;
        Ok(Tag {
            id: tag_id,
            name: name.into(),
            color: color.map(str::to_owned),
        })
    }
}

fn storage(error: sqlx::Error) -> SourceError {
    SourceError::Storage(error.to_string())
}
