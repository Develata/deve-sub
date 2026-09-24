//! Transaction helpers shared by fresh and not-modified source publication.

use deve_sub_domain::{ReconcileResult, SourceError};
use deve_sub_kernel::{SourceId, SourceRefreshJobId, Timestamp};
use sqlx::{Sqlite, Transaction};

/// Finish only the current lease, inside the transaction publishing its data.
pub(crate) async fn complete(
    tx: &mut Transaction<'_, Sqlite>,
    source_id: SourceId,
    job_id: SourceRefreshJobId,
    result: &ReconcileResult,
    not_modified: bool,
) -> Result<(), SourceError> {
    let now = crate::timestamp::format_ts(Timestamp::now()).map_err(SourceError::Storage)?;
    let changed = sqlx::query(
        "UPDATE source_refresh_jobs SET status = 'C', phase = 'publishing', finished_at = ?, \
         new_nodes = ?, duplicate_nodes = ?, reactivated_nodes = ?, missing_nodes = ?, \
         not_modified = ? WHERE id = ? AND source_id = ? AND status = 'R'",
    )
    .bind(now)
    .bind(result.new_nodes as i64)
    .bind(result.duplicate_nodes as i64)
    .bind(result.reactivated_nodes as i64)
    .bind(result.missing_nodes as i64)
    .bind(not_modified)
    .bind(job_id.to_string())
    .bind(source_id.to_string())
    .execute(&mut **tx)
    .await
    .map_err(|e| SourceError::Storage(e.to_string()))?;
    if changed.rows_affected() != 1 {
        return Err(SourceError::Storage(
            "refresh lease is no longer running".into(),
        ));
    }
    Ok(())
}
