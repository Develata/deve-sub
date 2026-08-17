//! Process-level advisory lock on the SQLite database file.
//!
//! SQLite's own `BEGIN IMMEDIATE` probe (used by `check_server_not_running`
//! in `backup.rs`) detects concurrent *write transactions* but is not a
//! authoritative process-level guard: two `deve-sub serve` processes can
//! both open a pool on the same DB file and interleave writes, leading to
//! `SQLITE_BUSY` errors at best and silent corruption at worst.
//!
//! This module provides an exclusive `flock` (via `fs2`, a safe wrapper
//! around `flock(2)`/`LockFileEx`) held for the lifetime of the owning
//! process. `serve` acquires it at startup; `restore` acquires it before
//! staging to guarantee the server is not running. The lock is released on
//! drop (process exit or explicit release).
//!
//! WHY `fs2` instead of raw libc: the workspace enforces
//! `unsafe_code = "forbid"`, so we cannot call `flock(2)` directly. `fs2`
//! encapsulates the `unsafe` syscall inside its own crate boundary, which
//! does not propagate to our workspace lint scope.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use anyhow::{Context, Result};
use fs2::FileExt;

/// An exclusive advisory lock on the database file.
///
/// The lock is held until this value is dropped. Callers must keep the
/// `DbLock` alive for the duration of the protected operation.
pub struct DbLock {
    _file: File,
}

impl DbLock {
    /// Acquire an exclusive (`LOCK_EX`) advisory lock on the database file.
    ///
    /// Creates the file if it does not exist (matching SQLite's `?mode=rwc`
    /// semantics) so a fresh install can lock before `open_db`.
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened or the lock cannot be
    /// acquired (another process holds it).
    pub fn acquire_exclusive(db_path: &Path) -> Result<Self> {
        ensure_parent(db_path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(db_path)
            .with_context(|| format!("failed to open DB file for lock: {}", db_path.display()))?;
        file.lock_exclusive()
            .map_err(|e| map_lock_error(e, db_path))?;
        Ok(Self { _file: file })
    }
}

/// Map a lock error to a user-friendly message distinguishing "another
/// process holds the lock" from generic I/O failures.
fn map_lock_error(e: io::Error, db_path: &Path) -> anyhow::Error {
    if e.kind() == io::ErrorKind::WouldBlock {
        anyhow::anyhow!(
            "another deve-sub process is already using the database at {} — \
             stop it before starting a new instance or running restore",
            db_path.display()
        )
    } else {
        anyhow::anyhow!(
            "failed to acquire database lock on {}: {e}",
            db_path.display()
        )
    }
}

/// Create the parent directory of the DB file if it does not exist.
fn ensure_parent(db_path: &Path) -> Result<()> {
    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create DB directory: {}", parent.display()))?;
    }
    Ok(())
}
