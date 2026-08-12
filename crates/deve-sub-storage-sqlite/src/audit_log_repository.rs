//! SQLite implementation of [`AuditLogRepository`].
//!
//! The `audit_log` table was created in migration 0002. This adapter
//! implements the append-only insert and filtered list with cursor
//! pagination. See ADR-0002 for the storage Port decision.

use async_trait::async_trait;
use deve_sub_domain::{AuditError, AuditLog, AuditLogFilter, AuditLogRepository};
use deve_sub_kernel::{AuditLogId, UserId};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

/// SQLite-backed audit log repository.
pub struct SqliteAuditLogRepository {
    pool: SqlitePool,
}

impl SqliteAuditLogRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct AuditLogRow {
    id: String,
    actor_id: Option<String>,
    action: String,
    target_type: Option<String>,
    target_id: Option<String>,
    details_json: Option<String>,
    created_at: String,
}

impl AuditLogRow {
    fn to_domain(&self) -> Result<AuditLog, AuditError> {
        Ok(AuditLog {
            id: AuditLogId::parse(&self.id).map_err(|e| AuditError::Storage(e.to_string()))?,
            actor_id: self
                .actor_id
                .as_deref()
                .map(UserId::parse)
                .transpose()
                .map_err(|e| AuditError::Storage(e.to_string()))?,
            action: self.action.clone(),
            target_type: self.target_type.clone(),
            target_id: self.target_id.clone(),
            details_json: self.details_json.clone(),
            created_at: parse_ts(&self.created_at).map_err(AuditError::Storage)?,
        })
    }
}

#[async_trait]
impl AuditLogRepository for SqliteAuditLogRepository {
    async fn insert(&self, entry: &AuditLog) -> Result<(), AuditError> {
        let actor_id = entry.actor_id.as_ref().map(|id| id.to_string());
        let created_at = format_ts(entry.created_at).map_err(AuditError::Storage)?;

        sqlx::query(
            "INSERT INTO audit_log (id, actor_id, action, target_type, target_id, details_json, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(entry.id.to_string())
        .bind(actor_id)
        .bind(&entry.action)
        .bind(&entry.target_type)
        .bind(&entry.target_id)
        .bind(&entry.details_json)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AuditError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn list(
        &self,
        filter: &AuditLogFilter,
        cursor: Option<AuditLogId>,
        limit: u32,
    ) -> Result<Vec<AuditLog>, AuditError> {
        // WHY: cap at 100 to prevent unbounded result sets.
        let limit = limit.min(100) as i64;

        // Build a dynamic query: each filter adds a WHERE clause, the
        // cursor adds `id < ?`, and the result is ordered newest-first.
        let mut conditions: Vec<String> = Vec::new();
        let mut query = String::from(
            "SELECT id, actor_id, action, target_type, target_id, details_json, created_at \
             FROM audit_log",
        );

        if filter.actor_id.is_some() {
            conditions.push("actor_id = ?".to_owned());
        }
        if filter.action.is_some() {
            conditions.push("action = ?".to_owned());
        }
        if filter.target_type.is_some() {
            conditions.push("target_type = ?".to_owned());
        }
        if filter.target_id.is_some() {
            conditions.push("target_id = ?".to_owned());
        }
        if cursor.is_some() {
            conditions.push("id < ?".to_owned());
        }

        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }
        query.push_str(" ORDER BY id DESC LIMIT ?");

        let mut q = sqlx::query_as::<_, AuditLogRow>(&query);

        if let Some(ref actor_id) = filter.actor_id {
            q = q.bind(actor_id.to_string());
        }
        if let Some(ref action) = filter.action {
            q = q.bind(action);
        }
        if let Some(ref target_type) = filter.target_type {
            q = q.bind(target_type);
        }
        if let Some(ref target_id) = filter.target_id {
            q = q.bind(target_id);
        }
        if let Some(c) = cursor {
            q = q.bind(c.to_string());
        }
        q = q.bind(limit);

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AuditError::Storage(e.to_string()))?;

        rows.iter().map(|r| r.to_domain()).collect()
    }
}
