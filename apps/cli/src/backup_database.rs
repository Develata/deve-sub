//! SQLite snapshot and verification queries used by backup and restore.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::COUNTED_TABLES;

pub(crate) async fn current_schema_version(pool: &sqlx::sqlite::SqlitePool) -> Result<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .context("failed to query schema version")?;
    Ok(row.0)
}

/// Inspect both the live database and immutable snapshot: refuse plaintext
/// cache archives and keep the manifest paired with the database's bound key.
pub(crate) async fn cache_backup_fingerprint(
    pool: &sqlx::sqlite::SqlitePool,
    supplied: Option<&str>,
) -> Result<Option<String>> {
    let version = current_schema_version(pool).await?;
    let has_cache = if version >= 7 {
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM generation_cache)")
            .fetch_one(pool)
            .await?
    } else {
        false
    };
    if has_cache && version < 29 {
        bail!(
            "legacy generation cache requires `deve-sub migrate --key-path <existing-key>` before backup"
        );
    }
    if version >= 29 {
        deve_sub_storage_sqlite::verify_generation_cache_encryption(pool).await?;
    }
    let bound: Option<String> = if version >= 18 {
        sqlx::query_scalar("SELECT current_key_fingerprint FROM key_metadata WHERE id = 1")
            .fetch_optional(pool)
            .await?
    } else {
        None
    };
    if let (Some(expected), Some(actual)) = (&bound, supplied)
        && expected != actual
    {
        bail!("master key fingerprint mismatch — refusing backup with a different key");
    }
    if has_cache && bound.is_none() {
        bail!(
            "encrypted generation cache has no master key binding — run `deve-sub migrate --key-path <existing-key>`"
        );
    }
    Ok(bound.or_else(|| supplied.map(str::to_owned)))
}

/// Validate the archived history without applying migrations or trusting a
/// manifest's version alone. Older archives must contain a complete prefix.
pub(crate) async fn verify_archived_schema(
    pool: &sqlx::sqlite::SqlitePool,
    manifest_version: i64,
) -> Result<()> {
    let actual = current_schema_version(pool).await?;
    if actual != manifest_version {
        bail!(
            "backup schema version mismatch: manifest records {manifest_version}, database records {actual}"
        );
    }
    match deve_sub_storage_sqlite::verify_schema(pool).await {
        Ok(()) => Ok(()),
        Err(deve_sub_storage_sqlite::StorageError::SchemaBehind { missing })
            if missing.iter().all(|version| *version > actual) =>
        {
            Ok(())
        }
        Err(error) => Err(error).context("backup database migration history is invalid"),
    }
}

/// Check the immutable snapshot's binding. Unlike ensure_key_binding this is
/// read-only, including on legacy snapshots that do not carry a binding.
pub(crate) async fn verify_key_binding(
    pool: &sqlx::sqlite::SqlitePool,
    loaded_fingerprint: Option<&str>,
    manifest_fingerprint: Option<&str>,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'key_metadata')",
    )
    .fetch_one(pool)
    .await
    .context("failed to inspect snapshot key metadata")?;
    if !exists {
        if current_schema_version(pool).await? >= 18 {
            bail!("database key_metadata table is missing from a schema that requires it");
        }
        return Ok(());
    }
    let bindings: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, current_key_fingerprint FROM key_metadata")
            .fetch_all(pool)
            .await
            .context("failed to read snapshot key binding")?;
    let bound = match bindings.as_slice() {
        [] => return Ok(()),
        [(1, fingerprint)] if !fingerprint.is_empty() => fingerprint,
        _ => bail!("invalid database master key binding metadata"),
    };
    let Some(loaded) = loaded_fingerprint else {
        bail!(
            "database master key binding requires the original key; no master key was loaded. Provide --key-path or configure security.master_key_path"
        );
    };
    if loaded != bound {
        bail!("master key fingerprint mismatch with database binding; refusing backup or restore");
    }
    if let Some(manifest) = manifest_fingerprint
        && manifest != bound
    {
        bail!("backup manifest master key fingerprint mismatch with database binding");
    }
    Ok(())
}

pub(crate) async fn collect_row_counts(
    pool: &sqlx::sqlite::SqlitePool,
) -> Result<BTreeMap<String, i64>> {
    let mut counts = BTreeMap::new();
    for table in COUNTED_TABLES {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        match sqlx::query_as::<_, (i64,)>(sql.as_str())
            .fetch_one(pool)
            .await
        {
            Ok((count,)) => {
                counts.insert((*table).to_owned(), count);
            }
            Err(e) => {
                tracing::debug!(table, error = %e, "skipping row count for missing/inaccessible table");
            }
        }
    }
    Ok(counts)
}

pub(crate) async fn vacuum_into(pool: &sqlx::sqlite::SqlitePool, target: &Path) -> Result<()> {
    let target_str = target
        .to_str()
        .context("snapshot path is not valid UTF-8")?;
    // Defense-in-depth: VACUUM INTO uses single-quote string interpolation.
    // The target is currently an internal tempfile path, but reject quotes
    // to prevent SQL injection if the path ever becomes user-controlled.
    if target_str.contains('\'') {
        bail!("snapshot path contains a single quote — refusing to interpolate into VACUUM INTO");
    }
    let sql = format!("VACUUM INTO '{target_str}'");
    sqlx::query(&sql)
        .execute(pool)
        .await
        .context("VACUUM INTO failed")?;
    Ok(())
}

pub(crate) async fn integrity_check(pool: &sqlx::sqlite::SqlitePool) -> Result<String> {
    let row: (String,) = sqlx::query_as("PRAGMA integrity_check")
        .fetch_one(pool)
        .await
        .context("integrity_check query failed")?;
    Ok(row.0)
}

pub(crate) fn fsync_file(path: &Path) -> Result<()> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open {} for fsync", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to fsync {}", path.display()))?;
    Ok(())
}

/// fsync the parent directory so a rename/unlink is durable across crashes.
/// On non-Unix this is a no-op (POSIX directory fsync is the relevant case).
pub(crate) fn fsync_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let file = fs::File::open(dir)
            .with_context(|| format!("failed to open dir {} for fsync", dir.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to fsync dir {}", dir.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
    Ok(())
}
