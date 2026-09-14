//! Audit cleanup transaction: revalidation, exact deletion, and durable receipt.

use deve_sub_domain::{AUDIT_CLEANUP_BATCH, AuditCleanupPreview, AuditError, AuditLog};
use deve_sub_kernel::{AuditLogId, Timestamp};
use sqlx::{SqliteConnection, SqlitePool};

use crate::timestamp::format_ts;

pub(super) async fn insert(
    connection: &mut SqliteConnection,
    entry: &AuditLog,
) -> Result<(), AuditError> {
    sqlx::query("INSERT INTO audit_log (id, actor_id, action, target_type, target_id, details_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(entry.id.to_string()).bind(entry.actor_id.map(|id| id.to_string()))
        .bind(&entry.action).bind(&entry.target_type).bind(&entry.target_id)
        .bind(&entry.details_json).bind(format_ts(entry.created_at).map_err(AuditError::Storage)?)
        .execute(connection).await.map_err(storage)?;
    Ok(())
}

pub(super) async fn preview(
    connection: &mut SqliteConnection,
    before: Timestamp,
) -> Result<AuditCleanupPreview, AuditError> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM audit_log WHERE created_at < ? ORDER BY created_at, id LIMIT ?",
    )
    .bind(format_ts(before).map_err(AuditError::Storage)?)
    .bind((AUDIT_CLEANUP_BATCH + 1) as i64)
    .fetch_all(connection)
    .await
    .map_err(storage)?;
    let has_more = ids.len() > AUDIT_CLEANUP_BATCH;
    let entry_ids = ids
        .iter()
        .take(AUDIT_CLEANUP_BATCH)
        .map(|id| AuditLogId::parse(id).map_err(|e| AuditError::Storage(e.to_string())))
        .collect::<Result<_, _>>()?;
    Ok(AuditCleanupPreview {
        before,
        entry_ids,
        has_more,
    })
}

pub(super) async fn execute(
    pool: &SqlitePool,
    before: Timestamp,
    entry_ids: &[AuditLogId],
    receipt: &AuditLog,
) -> Result<(), AuditError> {
    if entry_ids.is_empty() || entry_ids.len() > AUDIT_CLEANUP_BATCH {
        return Err(AuditError::Invalid("invalid cleanup batch size".into()));
    }
    // WHY: acquire the write lock before revalidating, so concurrent cleanup
    // cannot change the candidate set between the check and DELETE (M10).
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.map_err(storage)?;
    let current = preview(&mut tx, before).await?;
    if current.entry_ids != entry_ids {
        return Err(AuditError::Conflict);
    }
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("DELETE FROM audit_log WHERE id IN (");
    let mut separated = query.separated(", ");
    for id in entry_ids {
        separated.push_bind(id.to_string());
    }
    separated.push_unseparated(")");
    let deleted = query
        .build()
        .execute(&mut *tx)
        .await
        .map_err(storage)?
        .rows_affected();
    if deleted != entry_ids.len() as u64 {
        return Err(AuditError::Conflict);
    }
    // Failure or cancellation before commit rolls back both data and receipt.
    insert(&mut tx, receipt).await?;
    tx.commit().await.map_err(storage)
}

fn storage(error: sqlx::Error) -> AuditError {
    AuditError::Storage(error.to_string())
}
