//! SQLite implementation of [`NodeOverrideRepository`].
//!
//! Handles the `node_overrides`, `tags`, and `node_tags` tables. The
//! [`NodePoolRepository`] read path LEFT JOINs `node_overrides` and
//! `node_tags` to reconstruct the effective [`NodePoolEntry`], so override
//! and tag data are returned via the pool query, not via separate calls to
//! this trait. See NODE-004 through NODE-010.

use async_trait::async_trait;
use deve_sub_domain::{NodeOverride, NodeOverrideRepository, SourceError, Tag};
use deve_sub_kernel::{NodeId, NodeOverrideId, TagId};
use sqlx::sqlite::SqlitePool;

/// SQLite-backed node override and tag repository.
pub struct SqliteNodeOverrideRepository {
    pool: SqlitePool,
}

impl SqliteNodeOverrideRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Row mapping for `node_overrides`.
#[derive(sqlx::FromRow)]
struct OverrideRow {
    id: String,
    node_id: String,
    display_name: Option<String>,
    region: Option<String>,
    enabled: Option<i64>,
    sni: Option<String>,
    skip_cert_verify: Option<i64>,
    fingerprint: Option<String>,
    sort_order: i64,
}

impl OverrideRow {
    fn to_domain(&self) -> Result<NodeOverride, SourceError> {
        Ok(NodeOverride {
            id: NodeOverrideId::parse(&self.id).map_err(|e| SourceError::Storage(e.to_string()))?,
            node_id: NodeId::parse(&self.node_id)
                .map_err(|e| SourceError::Storage(e.to_string()))?,
            display_name: self.display_name.clone(),
            region: self.region.clone(),
            enabled: self.enabled.map(|e| e != 0),
            sni: self.sni.clone(),
            skip_cert_verify: self.skip_cert_verify.map(|e| e != 0),
            fingerprint: self.fingerprint.clone(),
            sort_order: self.sort_order,
        })
    }
}

/// Row mapping for `tags`.
#[derive(sqlx::FromRow)]
struct TagRow {
    id: String,
    name: String,
    color: Option<String>,
}

impl TagRow {
    fn to_domain(&self) -> Result<Tag, SourceError> {
        Ok(Tag {
            id: TagId::parse(&self.id).map_err(|e| SourceError::Storage(e.to_string()))?,
            name: self.name.clone(),
            color: self.color.clone(),
        })
    }
}

#[async_trait]
impl NodeOverrideRepository for SqliteNodeOverrideRepository {
    async fn upsert_override(&self, ov: &NodeOverride) -> Result<(), SourceError> {
        let enabled_i = ov.enabled.map(i64::from);
        let skip_cert_verify_i = ov.skip_cert_verify.map(i64::from);
        sqlx::query(
            "INSERT INTO node_overrides \
             (id, node_id, display_name, region, enabled, sni, skip_cert_verify, fingerprint, sort_order) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(node_id) DO UPDATE SET \
             id = excluded.id, display_name = excluded.display_name, \
             region = excluded.region, enabled = excluded.enabled, \
             sni = excluded.sni, skip_cert_verify = excluded.skip_cert_verify, \
             fingerprint = excluded.fingerprint, sort_order = excluded.sort_order",
        )
        .bind(ov.id.to_string())
        .bind(ov.node_id.to_string())
        .bind(&ov.display_name)
        .bind(&ov.region)
        .bind(enabled_i)
        .bind(&ov.sni)
        .bind(skip_cert_verify_i)
        .bind(&ov.fingerprint)
        .bind(ov.sort_order)
        .execute(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn get_override(&self, node_id: NodeId) -> Result<Option<NodeOverride>, SourceError> {
        let row: Option<OverrideRow> = sqlx::query_as(
            "SELECT id, node_id, display_name, region, enabled, sni, \
             skip_cert_verify, fingerprint, sort_order \
             FROM node_overrides WHERE node_id = ?",
        )
        .bind(node_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn delete_override(&self, node_id: NodeId) -> Result<(), SourceError> {
        sqlx::query("DELETE FROM node_overrides WHERE node_id = ?")
            .bind(node_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn patch_override_region(
        &self,
        node_id: NodeId,
        region: Option<String>,
    ) -> Result<(), SourceError> {
        // WHY: generate a new NodeOverrideId for the INSERT path. On conflict
        // (override already exists), only the region column is updated; the
        // existing id and all other fields are preserved. Passing None for
        // region clears the manual region (NODE-006).
        let new_id = NodeOverrideId::new();
        sqlx::query(
            "INSERT INTO node_overrides (id, node_id, region) VALUES (?, ?, ?) \
             ON CONFLICT(node_id) DO UPDATE SET region = excluded.region",
        )
        .bind(new_id.to_string())
        .bind(node_id.to_string())
        .bind(&region)
        .execute(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn batch_set_enabled(
        &self,
        node_ids: &[NodeId],
        enabled: bool,
    ) -> Result<u64, SourceError> {
        let enabled_i = i64::from(enabled);
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        let mut count = 0u64;
        for node_id in node_ids {
            // WHY: each upsert affects exactly one row (insert or update),
            // so rows_affected() is 1 per iteration. A new NodeOverrideId is
            // generated for the INSERT path; on conflict only enabled is set.
            let new_id = NodeOverrideId::new();
            let result = sqlx::query(
                "INSERT INTO node_overrides (id, node_id, enabled) VALUES (?, ?, ?) \
                 ON CONFLICT(node_id) DO UPDATE SET enabled = excluded.enabled",
            )
            .bind(new_id.to_string())
            .bind(node_id.to_string())
            .bind(enabled_i)
            .execute(&mut *tx)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
            count += result.rows_affected();
        }
        tx.commit()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(count)
    }

    async fn set_node_tags(&self, node_id: NodeId, tag_ids: &[TagId]) -> Result<(), SourceError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        sqlx::query("DELETE FROM node_tags WHERE node_id = ?")
            .bind(node_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        for tag_id in tag_ids {
            sqlx::query("INSERT INTO node_tags (node_id, tag_id) VALUES (?, ?)")
                .bind(node_id.to_string())
                .bind(tag_id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(|e| SourceError::Storage(e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn batch_set_tags(
        &self,
        assignments: &[(NodeId, Vec<TagId>)],
    ) -> Result<(), SourceError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        for (node_id, tag_ids) in assignments {
            sqlx::query("DELETE FROM node_tags WHERE node_id = ?")
                .bind(node_id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(|e| SourceError::Storage(e.to_string()))?;
            for tag_id in tag_ids {
                sqlx::query("INSERT INTO node_tags (node_id, tag_id) VALUES (?, ?)")
                    .bind(node_id.to_string())
                    .bind(tag_id.to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| SourceError::Storage(e.to_string()))?;
            }
        }
        tx.commit()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn list_tags(&self) -> Result<Vec<Tag>, SourceError> {
        let rows: Vec<TagRow> = sqlx::query_as("SELECT id, name, color FROM tags ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        rows.iter().map(|r| r.to_domain()).collect()
    }

    async fn create_tag(&self, name: &str, color: Option<&str>) -> Result<Tag, SourceError> {
        let id = TagId::new();
        sqlx::query("INSERT INTO tags (id, name, color) VALUES (?, ?, ?)")
            .bind(id.to_string())
            .bind(name)
            .bind(color)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("UNIQUE") {
                    SourceError::TagExists
                } else {
                    SourceError::Storage(msg)
                }
            })?;
        Ok(Tag {
            id,
            name: name.to_owned(),
            color: color.map(std::string::ToString::to_string),
        })
    }

    async fn delete_tag(&self, tag_id: TagId) -> Result<(), SourceError> {
        // WHY: ON DELETE CASCADE in migration 0004 removes node_tags rows
        // referencing this tag automatically. No manual cascade needed.
        let result = sqlx::query("DELETE FROM tags WHERE id = ?")
            .bind(tag_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SourceError::TagNotFound);
        }
        Ok(())
    }
}
