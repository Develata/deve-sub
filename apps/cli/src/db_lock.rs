//! Process-level advisory lock on the SQLite database, decoupled from the
//! DB file's inode.
//!
//! SQLite's own `BEGIN IMMEDIATE` probe (used by `check_server_not_running`
//! in `backup.rs`) detects concurrent *write transactions* but is not an
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
//! # DS-AUD-B05: sidecar lock file + bounded timeout
//!
//! Two prior defects (audit B-05):
//!
//! 1. **Blocking forever.** `lock_exclusive()` is a blocking syscall with
//!    no timeout. A second `serve`/`restore` against an already-locked DB
//!    hung indefinitely — no error, no log, no exit. Fixed by using
//!    `try_lock_exclusive()` in a bounded retry loop.
//!
//! 2. **Wrong inode after restore rename.** Locking the DB file itself
//!    meant `restore`'s atomic `fs::rename(staging, db_path)` left the
//!    old `serve`'s flock attached to the *old* inode (now at
//!    `.pre-restore`), while the new inode at `db_path` carried no lock.
//!    A post-restore `serve` would acquire a fresh lock on the unlocked
//!    new inode — two processes believing they hold the exclusive lock
//!    simultaneously. Fixed by locking a **sidecar** file
//!    `<db_path>.deve-sub.lock` that `rename` never touches, keeping the
//!    lock inode stable across restores.
//!
//! WHY `fs2` instead of raw libc: the workspace enforces
//! `unsafe_code = "forbid"`, so we cannot call `flock(2)` directly. `fs2`
//! encapsulates the `unsafe` syscall inside its own crate boundary, which
//! does not propagate to our workspace lint scope.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use fs2::FileExt;

/// Polling interval for the bounded `try_lock_exclusive` retry loop.
const BACKOFF: Duration = Duration::from_millis(100);

/// An exclusive advisory lock on the database, held on a sidecar file.
///
/// The lock is held until this value is dropped. Callers must keep the
/// `DbLock` alive for the duration of the protected operation.
///
/// WHY the sidecar file is not deleted on drop: removing it creates a
/// race where process B removes process A's lock file after A crashes
/// mid-drop, allowing C to create a fresh unlocked file and bypass
/// exclusion. Leaving the file is safe — the next acquire truncates and
/// rewrites the metadata, and the flock is the authoritative guard, not
/// the file's existence. `fs2` releases the flock when the `File` handle
/// drops.
#[derive(Debug)]
pub struct DbLock {
    _file: File,
}

impl DbLock {
    /// Acquire an exclusive lock on the sidecar lock file, waiting up to
    /// `timeout` for a held lock to be released.
    ///
    /// Creates the sidecar file if it does not exist. On success, writes
    /// holder metadata (PID, start time, binary version) into the lock
    /// file for operator diagnostics on a later conflict.
    ///
    /// # Errors
    /// Returns an error if the lock cannot be acquired within `timeout`
    /// (another process holds it), or if the file cannot be opened or
    /// locked for a non-contention reason.
    pub fn acquire_with_timeout(db_path: &Path, timeout: Duration) -> Result<Self> {
        let lock_path = sidecar_lock_path(db_path);
        ensure_parent(&lock_path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("failed to open lock file: {}", lock_path.display()))?;

        let deadline = Instant::now() + timeout;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => break,
                Err(e) if is_would_block(&e) => {
                    if Instant::now() >= deadline {
                        return Err(held_by_another_process(&lock_path, &mut file));
                    }
                    std::thread::sleep(BACKOFF);
                }
                Err(e) => {
                    return Err(map_lock_error(e, &lock_path));
                }
            }
        }

        write_holder_metadata(&mut file)?;
        Ok(Self { _file: file })
    }
}

/// Derive the sidecar lock path: `<db_path>.deve-sub.lock`.
///
/// Appends the suffix to the *full* db_path (preserving the extension),
/// so `db.sqlite` → `db.sqlite.deve-sub.lock`. The sidecar is a sibling
/// of the DB so `fs::rename(staging, db_path)` never touches it.
fn sidecar_lock_path(db_path: &Path) -> PathBuf {
    let mut p = db_path.as_os_str().to_owned();
    p.push(".deve-sub.lock");
    PathBuf::from(p)
}

/// Write a single holder-metadata line into the lock file for diagnostics.
///
/// Format: `pid=<pid> started=<rfc3339> bin=deve-sub/<version>\n`
/// Truncates the file to the new content (overwrites any stale holder
/// line from a crashed previous owner).
fn write_holder_metadata(file: &mut File) -> Result<()> {
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .context("failed to format current time as RFC3339")?;
    let pid = std::process::id();
    let bin = format!("deve-sub/{}", env!("CARGO_PKG_VERSION"));
    let line = format!("pid={pid} started={now} bin={bin}\n");

    file.seek(SeekFrom::Start(0))
        .context("failed to seek lock file to start")?;
    file.set_len(0).context("failed to truncate lock file")?;
    file.write_all(line.as_bytes())
        .context("failed to write holder metadata to lock file")?;
    file.sync_all()
        .context("failed to fsync lock file after writing metadata")?;
    Ok(())
}

/// Read the holder-metadata line currently in the lock file, if any.
/// Used to enrich the "held by another process" error message.
fn read_holder_metadata(file: &mut File) -> Option<String> {
    let mut buf = String::new();
    file.seek(SeekFrom::Start(0)).ok()?;
    buf.reserve(256);
    file.read_to_string(&mut buf).ok()?;
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Build the "held by another process" error, including the existing
/// holder metadata when readable so the operator knows *which* process
/// to stop.
fn held_by_another_process(lock_path: &Path, file: &mut File) -> anyhow::Error {
    let holder = read_holder_metadata(file);
    match holder {
        Some(meta) => anyhow::anyhow!(
            "another deve-sub process holds the lock on {}:\n  {meta}\n\
             stop it before starting a new instance or running restore",
            lock_path.display()
        ),
        None => anyhow::anyhow!(
            "another deve-sub process holds the lock on {} — \
             stop it before starting a new instance or running restore",
            lock_path.display()
        ),
    }
}

/// Detect whether a `try_lock_exclusive` error means "contended" vs a
/// genuine I/O failure. `fs2` returns `WouldBlock` on contention on both
/// Unix (`EAGAIN`/`EWOULDBLOCK`) and Windows.
fn is_would_block(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::WouldBlock
}

/// Map a non-contention lock error to a user-friendly message.
fn map_lock_error(e: io::Error, lock_path: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "failed to acquire database lock on {}: {e}",
        lock_path.display()
    )
}

/// Create the parent directory of the lock file if it does not exist.
fn ensure_parent(lock_path: &Path) -> Result<()> {
    if let Some(parent) = lock_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create lock directory: {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use std::thread;
    use std::time::Duration;

    /// A temp dir that auto-cleans, including the sidecar lock file.
    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    /// Asserts a `Duration` is within `[lo, hi]` (inclusive).
    fn within(elapsed: Duration, lo: Duration, hi: Duration) {
        assert!(
            elapsed >= lo && elapsed <= hi,
            "elapsed {elapsed:?} not within [{lo:?}, {hi:?}]"
        );
    }

    #[test]
    fn second_acquire_fails_within_timeout() {
        let dir = tmp();
        let db = dir.path().join("db.sqlite");

        let lock_a = DbLock::acquire_with_timeout(&db, Duration::from_secs(1))
            .expect("first acquire succeeds");

        let db_clone = db.clone();
        let handle = thread::spawn(move || {
            let start = Instant::now();
            let res = DbLock::acquire_with_timeout(&db_clone, Duration::from_millis(300));
            (start.elapsed(), res)
        });

        let (elapsed, res) = handle.join().expect("thread join");
        assert!(res.is_err(), "second acquire must fail while first is held");
        // 300ms timeout with 100ms backoff → must return well under 1s.
        // (Would hang forever under the old blocking `lock_exclusive`.)
        within(
            elapsed,
            Duration::from_millis(250),
            Duration::from_millis(900),
        );

        drop(lock_a);
    }

    #[test]
    fn lock_survives_rename_of_db_file() {
        // DS-AUD-B05 core regression: the lock must be on the sidecar, not
        // the DB inode, so renaming the DB (as `restore` does) must NOT
        // free the lock. The DB file is created to make the rename
        // realistic; the sidecar lock is what we actually assert against.
        let dir = tmp();
        let db = dir.path().join("db.sqlite");
        let db_renamed = dir.path().join("db.sqlite.old");

        std::fs::write(&db, b"dummy db content").expect("create db file");
        let lock = DbLock::acquire_with_timeout(&db, Duration::from_secs(1))
            .expect("acquire on original path");
        std::fs::rename(&db, &db_renamed).expect("rename db");

        // The sidecar `<db>.deve-sub.lock` still exists at the original
        // path and is still flock-locked by `lock`. A new acquire against
        // the *same* db_path must still fail — proving the lock did not
        // follow the renamed inode.
        let res = DbLock::acquire_with_timeout(&db, Duration::from_millis(200));
        assert!(
            res.is_err(),
            "acquire after rename must still fail — lock is on the sidecar, not the DB inode"
        );

        drop(lock);
    }

    #[test]
    fn release_on_drop_allows_reacquire() {
        let dir = tmp();
        let db = dir.path().join("db.sqlite");

        {
            let _lock =
                DbLock::acquire_with_timeout(&db, Duration::from_secs(1)).expect("first acquire");
        } // dropped here

        let _lock2 = DbLock::acquire_with_timeout(&db, Duration::from_secs(1))
            .expect("reacquire after drop succeeds");
    }

    #[test]
    fn holder_metadata_written() {
        let dir = tmp();
        let db = dir.path().join("db.sqlite");

        let lock = DbLock::acquire_with_timeout(&db, Duration::from_secs(1)).expect("acquire");
        let lock_path = sidecar_lock_path(&db);
        let content = std::fs::read_to_string(&lock_path).expect("read lock file");
        assert!(
            content.contains("pid="),
            "lock file must record pid: {content}"
        );
        assert!(
            content.contains("bin=deve-sub/"),
            "lock file must record bin: {content}"
        );
        assert!(
            content.contains("started="),
            "lock file must record started: {content}"
        );

        drop(lock);
    }

    #[test]
    fn error_message_includes_holder_metadata() {
        let dir = tmp();
        let db = dir.path().join("db.sqlite");

        let lock_a =
            DbLock::acquire_with_timeout(&db, Duration::from_secs(1)).expect("first acquire");

        let err = DbLock::acquire_with_timeout(&db, Duration::from_millis(100))
            .expect_err("second acquire fails");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("another deve-sub process holds the lock"),
            "msg: {msg}"
        );
        // The holder metadata (with our own pid) must be surfaced.
        let pid = std::process::id();
        assert!(
            msg.contains(&format!("pid={pid}")),
            "msg must include holder pid: {msg}"
        );

        drop(lock_a);
    }

    #[test]
    fn sidecar_lock_path_appends_suffix() {
        let p = sidecar_lock_path(Path::new("/var/lib/deve-sub/db.sqlite"));
        assert_eq!(
            p,
            PathBuf::from("/var/lib/deve-sub/db.sqlite.deve-sub.lock")
        );
    }
}
