//! SQLite implementation of [`SourceRefreshJobRepository`] (B-15).
//!
//! The per-source lease is enforced by a partial UNIQUE index
//! `idx_refresh_jobs_lease` on `(source_id) WHERE status = 'R'`. Inserting
//! a second Running job for the same source fails with a SQLITE_CONSTRAINT
//! error, which is mapped to [`SourceError::RefreshInProgress`].

use async_trait::async_trait;
use deve_sub_domain::source::refresh_job::{
    RefreshPhase, SourceRefreshJob, SourceRefreshJobStatus,
};
use deve_sub_domain::{SourceError, SourceRefreshJobRepository};
use deve_sub_kernel::{SourceId, SourceRefreshJobId, Timestamp};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

pub struct SqliteSourceRefreshJobRepository {
    pool: SqlitePool,
}

impl SqliteSourceRefreshJobRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct JobRow {
    id: String,
    source_id: String,
    status: String,
    phase: String,
    started_at: String,
    finished_at: Option<String>,
    error_message: Option<String>,
    new_nodes: i64,
    duplicate_nodes: i64,
    reactivated_nodes: i64,
    missing_nodes: i64,
    not_modified: i64,
}

impl JobRow {
    fn to_domain(&self) -> Result<SourceRefreshJob, SourceError> {
        let status = SourceRefreshJobStatus::from_db_char(&self.status)
            .ok_or_else(|| SourceError::Storage(format!("invalid job status '{}'", self.status)))?;
        let phase = RefreshPhase::from_db_str(&self.phase)
            .ok_or_else(|| SourceError::Storage(format!("invalid phase '{}'", self.phase)))?;
        let finished_at = match &self.finished_at {
            Some(s) => Some(parse_ts(s).map_err(SourceError::Storage)?),
            None => None,
        };
        Ok(SourceRefreshJob {
            id: SourceRefreshJobId::parse(&self.id)
                .map_err(|e| SourceError::Storage(e.to_string()))?,
            source_id: SourceId::parse(&self.source_id)
                .map_err(|e| SourceError::Storage(e.to_string()))?,
            status,
            phase,
            started_at: parse_ts(&self.started_at).map_err(SourceError::Storage)?,
            finished_at,
            error_message: self.error_message.clone(),
            new_nodes: u64::try_from(self.new_nodes)
                .map_err(|_| SourceError::Storage("negative new_nodes".to_owned()))?,
            duplicate_nodes: u64::try_from(self.duplicate_nodes)
                .map_err(|_| SourceError::Storage("negative duplicate_nodes".to_owned()))?,
            reactivated_nodes: u64::try_from(self.reactivated_nodes)
                .map_err(|_| SourceError::Storage("negative reactivated_nodes".to_owned()))?,
            missing_nodes: u64::try_from(self.missing_nodes)
                .map_err(|_| SourceError::Storage("negative missing_nodes".to_owned()))?,
            not_modified: self.not_modified != 0,
        })
    }
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .is_some_and(|db| db.code().is_some_and(|c| c == "1555" || c == "2067"))
}

#[async_trait]
impl SourceRefreshJobRepository for SqliteSourceRefreshJobRepository {
    async fn create(&self, job: &SourceRefreshJob) -> Result<(), SourceError> {
        let started_at = format_ts(job.started_at).map_err(SourceError::Storage)?;
        let result = sqlx::query(
            "INSERT INTO source_refresh_jobs \
             (id, source_id, status, phase, started_at, finished_at, error_message, \
              new_nodes, duplicate_nodes, reactivated_nodes, missing_nodes, not_modified) \
             VALUES (?, ?, ?, ?, ?, NULL, NULL, 0, 0, 0, 0, 0)",
        )
        .bind(job.id.to_string())
        .bind(job.source_id.to_string())
        .bind(job.status.as_db_char())
        .bind(job.phase.as_db_str())
        .bind(started_at)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_unique_violation(&e) => {
                Err(SourceError::RefreshInProgress(job.source_id.to_string()))
            }
            Err(e) => Err(SourceError::Storage(e.to_string())),
        }
    }

    async fn find_by_id(
        &self,
        id: SourceRefreshJobId,
    ) -> Result<Option<SourceRefreshJob>, SourceError> {
        let row: Option<JobRow> = sqlx::query_as(
            "SELECT id, source_id, status, phase, started_at, finished_at, error_message, \
             new_nodes, duplicate_nodes, reactivated_nodes, missing_nodes, not_modified \
             FROM source_refresh_jobs WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn find_running_for_source(
        &self,
        source_id: SourceId,
    ) -> Result<Option<SourceRefreshJob>, SourceError> {
        let row: Option<JobRow> = sqlx::query_as(
            "SELECT id, source_id, status, phase, started_at, finished_at, error_message, \
             new_nodes, duplicate_nodes, reactivated_nodes, missing_nodes, not_modified \
             FROM source_refresh_jobs WHERE source_id = ? AND status = 'R'",
        )
        .bind(source_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn mark_running(&self, id: SourceRefreshJobId) -> Result<(), SourceError> {
        let result = sqlx::query(
            "UPDATE source_refresh_jobs SET status = 'R' WHERE id = ? AND status = 'P'",
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await;

        match result {
            Ok(r) => {
                if r.rows_affected() == 0 {
                    return Err(SourceError::Storage(
                        "job not found or not in Pending status".to_owned(),
                    ));
                }
                Ok(())
            }
            Err(e) if is_unique_violation(&e) => Err(SourceError::RefreshInProgress(
                "lease held by another running job".to_owned(),
            )),
            Err(e) => Err(SourceError::Storage(e.to_string())),
        }
    }

    async fn update_phase(
        &self,
        id: SourceRefreshJobId,
        phase: RefreshPhase,
    ) -> Result<(), SourceError> {
        sqlx::query("UPDATE source_refresh_jobs SET phase = ? WHERE id = ?")
            .bind(phase.as_db_str())
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn mark_completed(
        &self,
        id: SourceRefreshJobId,
        new_nodes: u64,
        duplicate_nodes: u64,
        reactivated_nodes: u64,
        missing_nodes: u64,
        not_modified: bool,
    ) -> Result<(), SourceError> {
        let now = format_ts(deve_sub_kernel::Timestamp::now()).map_err(SourceError::Storage)?;
        sqlx::query(
            "UPDATE source_refresh_jobs \
             SET status = 'C', phase = 'publishing', finished_at = ?, \
             new_nodes = ?, duplicate_nodes = ?, reactivated_nodes = ?, \
             missing_nodes = ?, not_modified = ? WHERE id = ? AND status = 'R'",
        )
        .bind(now)
        .bind(new_nodes as i64)
        .bind(duplicate_nodes as i64)
        .bind(reactivated_nodes as i64)
        .bind(missing_nodes as i64)
        .bind(not_modified as i64)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn mark_failed(
        &self,
        id: SourceRefreshJobId,
        error_message: &str,
    ) -> Result<(), SourceError> {
        let now = format_ts(deve_sub_kernel::Timestamp::now()).map_err(SourceError::Storage)?;
        sqlx::query(
            "UPDATE source_refresh_jobs \
             SET status = 'F', finished_at = ?, error_message = ? \
             WHERE id = ? AND status IN ('P', 'R')",
        )
        .bind(now)
        .bind(error_message)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn mark_cancelled(&self, id: SourceRefreshJobId) -> Result<(), SourceError> {
        let now = format_ts(deve_sub_kernel::Timestamp::now()).map_err(SourceError::Storage)?;
        sqlx::query(
            "UPDATE source_refresh_jobs \
             SET status = 'X', finished_at = ? \
             WHERE id = ? AND status IN ('P', 'R')",
        )
        .bind(now)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn list_for_source(
        &self,
        source_id: SourceId,
        limit: u32,
    ) -> Result<Vec<SourceRefreshJob>, SourceError> {
        let limit = limit.min(100) as i64;
        let rows: Vec<JobRow> = sqlx::query_as(
            "SELECT id, source_id, status, phase, started_at, finished_at, error_message, \
             new_nodes, duplicate_nodes, reactivated_nodes, missing_nodes, not_modified \
             FROM source_refresh_jobs WHERE source_id = ? ORDER BY started_at DESC LIMIT ?",
        )
        .bind(source_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        rows.iter().map(|r| r.to_domain()).collect()
    }

    async fn recover_crashed_jobs(&self) -> Result<u64, SourceError> {
        let now = format_ts(deve_sub_kernel::Timestamp::now()).map_err(SourceError::Storage)?;
        let result = sqlx::query(
            "UPDATE source_refresh_jobs \
             SET status = 'F', finished_at = ?, error_message = 'process crashed during refresh' \
             WHERE status = 'P' OR status = 'R'",
        )
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(result.rows_affected())
    }

    async fn recover_stale_jobs(
        &self,
        cutoff: Timestamp,
        reason: &str,
    ) -> Result<u64, SourceError> {
        let cutoff_str = format_ts(cutoff).map_err(SourceError::Storage)?;
        let now = format_ts(deve_sub_kernel::Timestamp::now()).map_err(SourceError::Storage)?;
        let result = sqlx::query(
            "UPDATE source_refresh_jobs \
             SET status = 'F', finished_at = ?, error_message = ? \
             WHERE status = 'R' AND started_at < ?",
        )
        .bind(now)
        .bind(reason)
        .bind(cutoff_str)
        .execute(&self.pool)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(result.rows_affected())
    }
}
