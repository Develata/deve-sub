//! SQLite operational maintenance; deliberately outside business Ports.

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

use crate::StorageError;

/// Result of a non-blocking WAL checkpoint and filesystem resource sample.
#[derive(Debug, Clone, Copy)]
pub struct WalSample {
    /// SQLite busy result (PASSIVE may leave frames even when this is zero).
    pub busy: i64,
    /// Frames currently in the WAL, or -1 if no WAL exists.
    pub log_frames: i64,
    /// Frames backfilled into the main database, or -1 if no WAL exists.
    pub checkpointed_frames: i64,
    /// Main database bytes, when filesystem metadata is available.
    pub database_bytes: Option<u64>,
    /// WAL bytes; absent WAL is zero, inaccessible metadata is unknown.
    pub wal_bytes: Option<u64>,
}

/// SQLite-specific checkpoint owner, wired and scheduled by the CLI.
///
/// PASSIVE never waits for readers/writers to finish. A long reader may pin
/// frames; size and outstanding-frame telemetry exposes that condition.
/// Auto-checkpointing remains enabled. No periodic TRUNCATE is performed.
pub struct SqliteMaintenance {
    pool: SqlitePool,
    database_path: PathBuf,
    wal_path: PathBuf,
}

impl SqliteMaintenance {
    /// Construct maintenance for the same pool and path used by `serve`.
    #[must_use]
    pub fn new(pool: SqlitePool, path: impl AsRef<Path>) -> Self {
        let database_path = path.as_ref().to_path_buf();
        let mut wal_path = database_path.as_os_str().to_owned();
        wal_path.push("-wal");
        Self {
            pool,
            database_path,
            wal_path: wal_path.into(),
        }
    }

    /// Prune one bounded batch of expired operational history per table.
    pub async fn prune_history(&self) -> Result<u64, StorageError> {
        crate::retention::prune(&self.pool).await
    }

    /// Perform a PASSIVE checkpoint and report counters and file sizes.
    ///
    /// Failure leaves the WAL authoritative; callers log and retry on the
    /// next maintenance tick. PASSIVE success alone does not prove all frames
    /// were copied: compare `log_frames` with `checkpointed_frames`.
    pub async fn checkpoint(&self) -> Result<WalSample, StorageError> {
        let (busy, log_frames, checkpointed_frames): (i64, i64, i64) =
            sqlx::query_as("PRAGMA wal_checkpoint(PASSIVE)")
                .fetch_one(&self.pool)
                .await?;
        let sample = WalSample {
            busy,
            log_frames,
            checkpointed_frames,
            database_bytes: file_size(&self.database_path).await,
            wal_bytes: file_size(&self.wal_path).await,
        };
        tracing::info!(
            busy,
            log_frames,
            checkpointed_frames,
            database_bytes = sample.database_bytes,
            wal_bytes = sample.wal_bytes,
            "sqlite checkpoint"
        );
        if busy != 0 || log_frames > checkpointed_frames {
            tracing::warn!(
                busy,
                log_frames,
                checkpointed_frames,
                "sqlite WAL checkpoint incomplete"
            );
        }
        Ok(sample)
    }
}

async fn file_size(path: &Path) -> Option<u64> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => Some(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(0),
        Err(error) => {
            tracing::warn!(%error, "sqlite resource metadata unavailable");
            None
        }
    }
}
