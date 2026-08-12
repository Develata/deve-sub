//! Backup and restore CLI commands (M11).
//!
//! `deve-sub backup --output <path>` creates a versioned tar archive
//! containing a consistent SQLite snapshot (`VACUUM INTO`), a manifest with
//! schema version and row counts, non-secret configuration, and metadata.
//!
//! `deve-sub restore --input <path>` reads the archive, refuses if the server
//! is running, restores the database, runs forward migrations if the backup
//! schema is older (constraint #13), and verifies integrity.
//!
//! See `docs/plan/milestones/M11-archive-and-snapshot.md`.
//!
//! # Security notes
//!
//! Backup archives contain the **full production database** — users, TOTP
//! secrets, session tokens, subscription URLs, and encrypted sensitive fields.
//! They also include hostname and OS metadata. Treat archives as sensitive:
//! store them with restrictive permissions and never distribute them off-box.
//!
//! The `check_server_not_running` guard assumes `journal_mode=WAL`. If the
//! server uses a different journal mode (e.g. DELETE), the WAL/shm files may
//! not exist even while the server holds the database open. Always stop the
//! server process before restoring.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::{Deserialize, Serialize};

use crate::commands::{ensure_db_dir, load_config, open_db};

/// Backup format version. Increment when the archive layout changes
/// incompatibly.
const BACKUP_FORMAT_VERSION: u32 = 1;

/// Arguments for `deve-sub backup`.
#[derive(Args)]
pub struct BackupArgs {
    /// Output path for the backup tar archive.
    #[arg(long)]
    pub output: PathBuf,

    /// Path to configuration file.
    #[arg(long, env = "DEVE_SUB_CONFIG")]
    pub config: Option<PathBuf>,

    /// Database path (overrides config).
    #[arg(long, env = "DEVE_SUB_DB_PATH")]
    pub db_path: Option<String>,
}

/// Arguments for `deve-sub restore`.
#[derive(Args)]
pub struct RestoreArgs {
    /// Input path of the backup tar archive.
    #[arg(long)]
    pub input: PathBuf,

    /// Path to configuration file.
    #[arg(long, env = "DEVE_SUB_CONFIG")]
    pub config: Option<PathBuf>,

    /// Database path to restore into (overrides config).
    #[arg(long, env = "DEVE_SUB_DB_PATH")]
    pub db_path: Option<String>,
}

/// Backup manifest — describes the archive contents and schema version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    /// Backup format version (currently 1).
    pub version: u32,
    /// Schema migration version at backup time (highest migration number).
    pub schema_version: i64,
    /// ISO 8601 timestamp when the backup was created.
    pub created_at: String,
    /// Row counts for key tables, used by restore verification.
    pub row_counts: BTreeMap<String, i64>,
}

/// Backup metadata — environment info, not application state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    /// Deve Sub binary version.
    pub deve_sub_version: String,
    /// Git commit hash at build time, if available.
    #[serde(default)]
    pub git_commit: String,
    /// Host name at backup time.
    pub host: String,
    /// OS string at backup time.
    pub os: String,
}

/// Tables whose row counts are recorded in the manifest for restore
/// verification. Ordered alphabetically for stable output.
const COUNTED_TABLES: &[&str] = &[
    "audit_log",
    "generation_cache",
    "latency_records",
    "node_overrides",
    "node_source_bindings",
    "node_tags",
    "nodes",
    "outbox_event",
    "pool_meta",
    "probe_runs",
    "probe_sources",
    "recovery_codes",
    "sessions",
    "source_items",
    "source_snapshots",
    "sources",
    "subscription_short_codes",
    "subscription_temp_links",
    "subscription_tokens",
    "subscription_traffic",
    "subscriptions",
    "tags",
    "template_versions",
    "templates",
    "totp_secrets",
    "traffic_daily_snapshots",
    "users",
];

/// Run the backup command: snapshot the database and write a versioned tar
/// archive to `args.output`.
pub async fn backup(args: BackupArgs) -> Result<()> {
    let config = load_config(&args.config)?;
    let db_path = args.db_path.unwrap_or_else(|| config.database.path.clone());

    if !Path::new(&db_path).exists() {
        bail!("database file not found: {db_path} — run `deve-sub migrate` first");
    }

    tracing::info!(db_path = %db_path, output = %args.output.display(), "starting backup");

    let pool = open_db(&db_path, 1).await?;

    let schema_version = current_schema_version(&pool).await?;
    let row_counts = collect_row_counts(&pool).await?;

    // During a normal backup with a fully-migrated DB, all COUNTED_TABLES
    // should be accessible. A missing table signals partial migration or
    // corruption. Skip this check when the DB schema is older than the
    // embedded version (the DB may not have been migrated yet, so some
    // tables legitimately don't exist).
    if schema_version == deve_sub_storage_sqlite::embedded_schema_version()
        && row_counts.len() < COUNTED_TABLES.len()
    {
        let missing: Vec<&str> = COUNTED_TABLES
            .iter()
            .filter(|t| !row_counts.contains_key(**t))
            .copied()
            .collect();
        bail!(
            "backup incomplete — {}/{} tables inaccessible, missing: [{}]. \
             Run `deve-sub migrate` and retry.",
            row_counts.len(),
            COUNTED_TABLES.len(),
            missing.join(", ")
        );
    }

    let snapshot_dir = tempfile::tempdir().context("failed to create temp dir for snapshot")?;
    let snapshot_path = snapshot_dir.path().join("database.sqlite");

    vacuum_into(&pool, &snapshot_path).await?;

    let manifest = BackupManifest {
        version: BACKUP_FORMAT_VERSION,
        schema_version,
        created_at: now_iso8601(),
        row_counts,
    };

    let metadata = BackupMetadata {
        deve_sub_version: env!("CARGO_PKG_VERSION").to_owned(),
        git_commit: option_env!("GIT_COMMIT").unwrap_or("unknown").to_owned(),
        host: hostname(),
        os: std::env::consts::OS.to_owned(),
    };

    let config_json =
        serde_json::to_string_pretty(&config).context("failed to serialize config")?;
    let manifest_json =
        serde_json::to_string_pretty(&manifest).context("failed to serialize manifest")?;
    let metadata_json =
        serde_json::to_string_pretty(&metadata).context("failed to serialize metadata")?;

    write_tar(
        &args.output,
        &snapshot_path,
        &manifest_json,
        &config_json,
        &metadata_json,
    )?;

    tracing::info!(
        output = %args.output.display(),
        schema_version,
        tables = manifest.row_counts.len(),
        "backup complete"
    );

    println!("Backup written to {}", args.output.display());
    println!("  schema version: {schema_version}");
    println!("  format version: {}", BACKUP_FORMAT_VERSION);
    println!("  tables counted: {}", manifest.row_counts.len());

    Ok(())
}

/// Run the restore command: read the archive, restore the database, run
/// forward migrations if needed, and verify integrity.
pub async fn restore(args: RestoreArgs) -> Result<()> {
    let config = load_config(&args.config)?;
    let db_path = args.db_path.unwrap_or_else(|| config.database.path.clone());

    if !args.input.exists() {
        bail!("backup file not found: {}", args.input.display());
    }

    tracing::info!(input = %args.input.display(), db_path = %db_path, "starting restore");

    let extract_dir = tempfile::tempdir().context("failed to create temp dir for extraction")?;
    let (manifest, snapshot_path) = extract_archive(&args.input, extract_dir.path())?;

    if manifest.version != BACKUP_FORMAT_VERSION {
        bail!(
            "backup format version {} is not supported (expected {})",
            manifest.version,
            BACKUP_FORMAT_VERSION
        );
    }

    let current_version = deve_sub_storage_sqlite::embedded_schema_version();
    if manifest.schema_version > current_version {
        bail!(
            "backup schema version {} is newer than this binary's schema version {} — \
             upgrade the binary to match or exceed the backup (constraint #13: forward-only)",
            manifest.schema_version,
            current_version
        );
    }

    check_server_not_running(&db_path)?;

    ensure_db_dir(&db_path)?;

    fs::copy(&snapshot_path, &db_path)
        .with_context(|| format!("failed to copy snapshot to database path {}", db_path))?;

    tracing::info!(db_path = %db_path, "database restored");

    let pool = open_db(&db_path, 1).await?;

    if manifest.schema_version < current_version {
        tracing::info!(
            backup_schema = manifest.schema_version,
            current_schema = current_version,
            "running forward migrations"
        );
        deve_sub_storage_sqlite::run_migrations(&pool).await?;
        tracing::info!("forward migrations applied");
    }

    verify_restore(&pool, &manifest).await?;

    let integrity = integrity_check(&pool).await?;
    if integrity != "ok" {
        bail!("database integrity check failed: {integrity}");
    }

    println!("Restore complete.");
    println!("  database: {db_path}");
    println!(
        "  schema version: {} -> {current_version}",
        manifest.schema_version
    );
    println!("  integrity: ok");

    Ok(())
}

async fn current_schema_version(pool: &sqlx::sqlite::SqlitePool) -> Result<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .context("failed to query schema version")?;
    Ok(row.0)
}

async fn collect_row_counts(pool: &sqlx::sqlite::SqlitePool) -> Result<BTreeMap<String, i64>> {
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

async fn vacuum_into(pool: &sqlx::sqlite::SqlitePool, target: &Path) -> Result<()> {
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

fn write_tar(
    output: &Path,
    snapshot_path: &Path,
    manifest_json: &str,
    config_json: &str,
    metadata_json: &str,
) -> Result<()> {
    let file = fs::File::create(output)
        .with_context(|| format!("failed to create backup file {}", output.display()))?;
    let mut builder = tar::Builder::new(file);

    add_file_bytes(&mut builder, "manifest.json", manifest_json.as_bytes())?;
    add_file_bytes(&mut builder, "config.json", config_json.as_bytes())?;
    add_file_bytes(&mut builder, "metadata.json", metadata_json.as_bytes())?;

    let snapshot_bytes = fs::read(snapshot_path).context("failed to read snapshot database")?;
    add_file_bytes(&mut builder, "database.sqlite", &snapshot_bytes)?;

    builder.finish().context("failed to finalize tar archive")?;
    Ok(())
}

fn add_file_bytes(builder: &mut tar::Builder<fs::File>, name: &str, data: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_path(name).context("invalid archive path")?;
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append(&header, data)
        .with_context(|| format!("failed to append {name} to archive"))?;
    Ok(())
}

fn extract_archive(archive_path: &Path, dest: &Path) -> Result<(BackupManifest, PathBuf)> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open backup archive {}", archive_path.display()))?;
    let mut archive = tar::Archive::new(file);

    let mut manifest: Option<BackupManifest> = None;
    let mut snapshot_found = false;

    for entry in archive
        .entries()
        .context("failed to read archive entries")?
    {
        let mut entry = entry.context("failed to read archive entry")?;
        let path = entry.path().context("invalid entry path")?.into_owned();
        let name = path.to_string_lossy().into_owned();

        // Defense-in-depth: reject absolute paths and parent-component
        // traversal regardless of the allowlist match below. The exact-name
        // allowlist is the primary guard; this ensures safety survives
        // future allowlist edits.
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            tracing::warn!(entry = %name, "skipping entry with unsafe path");
            continue;
        }

        match &name[..] {
            "manifest.json" => {
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf).context("read manifest")?;
                manifest = Some(serde_json::from_slice(&buf).context("parse manifest.json")?);
            }
            "database.sqlite" => {
                let target = dest.join("database.sqlite");
                entry.unpack(&target).context("unpack database.sqlite")?;
                snapshot_found = true;
            }
            "config.json" | "metadata.json" => {
                let _ = entry.unpack(dest.join(&*name));
            }
            _ => {
                tracing::warn!(entry = %name, "skipping unknown archive entry");
            }
        }
    }

    let manifest = manifest.context("archive missing manifest.json")?;
    if !snapshot_found {
        bail!("archive missing database.sqlite");
    }

    Ok((manifest, dest.join("database.sqlite")))
}

async fn verify_restore(pool: &sqlx::sqlite::SqlitePool, manifest: &BackupManifest) -> Result<()> {
    let actual_counts = collect_row_counts(pool).await?;

    let mut mismatches = Vec::new();
    for (table, expected) in &manifest.row_counts {
        let actual = actual_counts.get(table).copied().unwrap_or(0);
        if actual != *expected {
            mismatches.push(format!("{table}: expected {expected}, got {actual}"));
        }
    }

    if mismatches.is_empty() {
        tracing::info!("restore verification passed — all row counts match");
        Ok(())
    } else {
        bail!(
            "restore verification failed — row count mismatches:\n  {}",
            mismatches.join("\n  ")
        );
    }
}

async fn integrity_check(pool: &sqlx::sqlite::SqlitePool) -> Result<String> {
    let row: (String,) = sqlx::query_as("PRAGMA integrity_check")
        .fetch_one(pool)
        .await
        .context("integrity_check query failed")?;
    Ok(row.0)
}

/// Check whether the server appears to be running by probing for a SQLite
/// write lock. If a process holds the database open with an active
/// transaction, restoring would corrupt state.
fn check_server_not_running(db_path: &str) -> Result<()> {
    let path = Path::new(db_path);
    let wal = path.with_extension("db-wal");
    let shm = path.with_extension("db-shm");
    if wal.exists() || shm.exists() {
        bail!(
            "WAL/shm files for {db_path} exist — the server may be running. \
             Stop the server before restoring."
        );
    }
    Ok(())
}

fn now_iso8601() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_owned())
}
