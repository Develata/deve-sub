//! Durable private rollback pair, separate from publishable backup archives.
use crate::backup::database;
use anyhow::{Context, Result, bail};
use deve_sub_security::MasterKey;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(crate) async fn save(
    pool: &SqlitePool,
    parent: &Path,
    version: i64,
    key: Option<&MasterKey>,
) -> Result<PathBuf> {
    let directory = tempfile::Builder::new()
        .prefix(".deve-sub-pre-migrate-")
        .tempdir_in(parent)
        .context("create private rollback directory before migration")?;
    let snapshot = directory.path().join("database.sqlite");
    database::vacuum_into(pool, &snapshot).await?;
    restrict(&snapshot)?;
    sync_file(&snapshot)?;
    let check = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&snapshot)
                .read_only(true),
        )
        .await?;
    database::verify_archived_schema(&check, version).await?;
    if database::integrity_check(&check).await? != "ok" {
        bail!("rollback snapshot integrity failed");
    }
    check.close().await;
    let fingerprint = key.map(MasterKey::fingerprint).transpose()?;
    if let Some(key) = key {
        let path = directory.path().join("master.key");
        write_private(&path, key.as_bytes())?;
        let restored = MasterKey::load(&path).context("verify rollback original key bytes")?;
        if restored.as_bytes() != key.as_bytes() {
            bail!("rollback key verification failed");
        }
    }
    let manifest = serde_json::json!({"purpose":"private pre-migration rollback; may contain legacy plaintext", "schema_version":version, "database_sha256":hash_file(&snapshot)?, "master_key_fingerprint":fingerprint});
    write_private(
        &directory.path().join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    sync_dir(directory.path())?;
    // Keep completed recovery evidence even if subsequent staging fails.
    let saved = directory.keep();
    sync_dir(parent)?;
    Ok(saved)
}

/// Retain the already parsed bytes, not a racy second read of external files.
pub(super) fn save_security_evidence(
    directory: &Path,
    review: &super::security::PreparedReview,
) -> Result<()> {
    write_private(&directory.join("security-map.json"), &review.map_bytes)?;
    for (sha256, bytes) in &review.artifacts {
        write_private(&directory.join(format!("input-{sha256}.bin")), bytes)?;
    }
    let manifest = serde_json::json!({"plan_sha256":review.plan.plan_sha256,"artifacts":review.artifacts.keys().collect::<Vec<_>>(),"scope":"private original mapping and artifact bytes; contains sensitive node configuration"});
    write_private(
        &directory.join("security-evidence.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    sync_dir(directory)
}

pub(crate) fn sync_file(path: &Path) -> Result<()> {
    fs::File::open(path)?
        .sync_all()
        .context("sync migration file")
}
pub(crate) fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?
        .sync_all()
        .context("sync migration directory")?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub(crate) fn restrict(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
