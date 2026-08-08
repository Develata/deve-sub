//! SQLite implementation of [`PoolMetaRepository`].
//!
//! The pool revision is a singleton counter in the `pool_meta` table
//! (migration 0008). It is bumped on every node pool mutation so stale
//! generation cache entries are invalidated. See
//! `docs/plan/milestones/M5-generator-and-v3-template.md` §"Generation cache".

use async_trait::async_trait;
use deve_sub_domain::{PoolMetaRepository, SourceError};
use deve_sub_kernel::Revision;
use sqlx::sqlite::SqlitePool;

pub struct SqlitePoolMetaRepository {
    pool: SqlitePool,
}

impl SqlitePoolMetaRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PoolMetaRepository for SqlitePoolMetaRepository {
    async fn get_revision(&self) -> Result<Revision, SourceError> {
        let (rev,): (i64,) = sqlx::query_as("SELECT revision FROM pool_meta WHERE id = 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        let rev_u64 = u64::try_from(rev)
            .map_err(|_| SourceError::Storage("negative pool revision".to_owned()))?;
        Ok(Revision::new(rev_u64))
    }

    async fn bump_revision(&self) -> Result<Revision, SourceError> {
        let (rev,): (i64,) = sqlx::query_as(
            "UPDATE pool_meta SET revision = revision + 1 WHERE id = 1 \
             RETURNING revision",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        let rev_u64 = u64::try_from(rev)
            .map_err(|_| SourceError::Storage("negative pool revision".to_owned()))?;
        Ok(Revision::new(rev_u64))
    }
}
