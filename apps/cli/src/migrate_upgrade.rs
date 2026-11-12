//! Staged upgrade orchestration; no live schema is changed before verification.
use super::{MigrateArgs, preflight, security};
use crate::{
    backup::database,
    commands::{ensure_db_dir, open_db},
    db_lock::DbLock,
};
use anyhow::{Context, Result, bail};
use deve_sub_domain::source::recovery::NodeSecurityPlan;
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::SqliteNodePoolRepository;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[path = "migrate_rollback.rs"]
mod rollback;

pub(super) async fn run(args: MigrateArgs) -> Result<()> {
    let explicit_key = args
        .key_path
        .as_deref()
        .map(|p| MasterKey::load(Path::new(p)))
        .transpose()
        .context("load explicitly requested original master key before migration")?;
    ensure_db_dir(&args.db_path)?;
    let target = Path::new(&args.db_path);
    if std::fs::symlink_metadata(target).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!(
            "migration database must be a regular path, not a symlink; use its resolved path consistently"
        );
    }
    let _lock = DbLock::acquire_with_timeout(target, Duration::from_secs(5))?;
    if !target.exists() || std::fs::metadata(target)?.len() == 0 {
        if args.security_map.is_some() {
            bail!("security recovery requires an existing node database");
        }
        let pool = open_db(&args.db_path, 1).await?;
        ensure_current_schema(&pool).await?;
        pool.close().await;
        println!("Migrations applied successfully to {}", args.db_path);
        return Ok(());
    }
    let source = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(target)
                .read_only(true),
        )
        .await?;
    let tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(&source)
    .await?;
    if tables == 0 {
        if args.security_map.is_some() {
            bail!("security recovery requires an existing node database");
        }
        source.close().await;
        let pool = open_db(&args.db_path, 1).await?;
        ensure_current_schema(&pool).await?;
        pool.close().await;
        println!("Migrations applied successfully to {}", args.db_path);
        return Ok(());
    }
    let key = if explicit_key.is_some() {
        explicit_key
    } else if preflight::needs_key(&source).await? {
        Some(
            MasterKey::load(Path::new("data/master.key"))
                .context("upgrade requires the original master key; provide --key-path")?,
        )
    } else {
        None
    };
    let version = preflight::verify(&source, key.as_ref()).await?;
    if version < 30 && preflight::has_rows(&source, "nodes").await? && args.security_map.is_none() {
        bail!(
            "historical node pool requires --security-map with an explicit reviewed old-ID mapping (including reviewed empty actions when no affected IDs were confirmed)"
        );
    }
    let plan = match args.security_map.as_deref() {
        Some(path) => Some(
            security::prepare(
                &source,
                key.as_ref()
                    .context("security recovery requires the original master key")?,
                path,
            )
            .await?,
        ),
        None => None,
    };
    if plan.is_none()
        && deve_sub_storage_sqlite::verify_schema(&source)
            .await
            .is_ok()
        && deve_sub_storage_sqlite::verify_generation_cache_encryption(&source)
            .await
            .is_ok()
    {
        if preflight::has_rows(&source, "generation_cache").await? {
            deve_sub_storage_sqlite::verify_generation_cache_payloads(
                &source,
                key.as_ref().context("original cache key required")?,
            )
            .await?;
        }
        source.close().await;
        println!(
            "Database schema and cache are already current: {}",
            args.db_path
        );
        return Ok(());
    }
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let recovery = rollback::save(
        &source,
        args.rollback_dir.as_deref().unwrap_or(parent),
        version,
        key.as_ref(),
    )
    .await?;
    if let Some(review) = &plan {
        rollback::save_security_evidence(&recovery, review)?;
    }
    source.close().await;
    let stage = tempfile::Builder::new()
        .prefix(".deve-sub-migrate-")
        .tempdir_in(parent)?;
    let staged_path = stage.path().join("database.sqlite");
    std::fs::copy(recovery.join("database.sqlite"), &staged_path)?;
    rollback::restrict(&staged_path)?;
    let pool = open_db(&staged_path.to_string_lossy(), 1).await?;
    let result = upgrade_staged(
        &pool,
        key.as_ref(),
        plan.as_ref().map(|review| &review.plan),
    )
    .await;
    if result.is_ok() {
        checkpoint(&pool).await?;
    }
    pool.close().await;
    let impact = result.with_context(|| {
        format!(
            "staged upgrade failed; live database unchanged; private rollback pair at {}",
            recovery.display()
        )
    })?;
    if let Some(impact) = impact {
        println!("{}", serde_json::to_string(&impact)?);
    }
    if args.preview {
        println!(
            "Preview verified on private staging; live database unchanged. Private rollback pair: {}",
            recovery.display()
        );
        return Ok(());
    }
    rollback::sync_file(&staged_path)?;
    // WHY: old WAL pages must not be replayed into the newly renamed schema.
    // Management writers share this sidecar lock. Checkpoint and close the old
    // DB before removing sidecars and publishing the staged database.
    let live = open_db(&args.db_path, 1).await?;
    checkpoint(&live).await?;
    live.close().await;
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", target.display()));
        match std::fs::remove_file(sidecar) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    std::fs::rename(&staged_path, target)
        .context("publish staged migration; original rollback pair remains available")?;
    rollback::sync_dir(parent)?;
    println!("Migrations applied successfully to {}", args.db_path);
    println!("Private rollback pair: {}", recovery.display());
    Ok(())
}

async fn upgrade_staged(
    pool: &SqlitePool,
    key: Option<&MasterKey>,
    plan: Option<&NodeSecurityPlan>,
) -> Result<Option<serde_json::Value>> {
    let old_version = preflight::verify(pool, key).await?;
    let before = database::collect_row_counts(pool).await?;
    deve_sub_storage_sqlite::run_migrations(pool).await?;
    if preflight::has_rows(pool, "generation_cache").await? {
        deve_sub_storage_sqlite::migrate_generation_cache(
            pool,
            key.context("cache migration requires original key")?,
        )
        .await?;
    }
    deve_sub_storage_sqlite::verify_schema(pool).await?;
    deve_sub_storage_sqlite::verify_generation_cache_encryption(pool).await?;
    let fingerprint = key.map(MasterKey::fingerprint).transpose()?;
    database::verify_key_binding(pool, fingerprint.as_deref(), None).await?;
    let after = database::collect_row_counts(pool).await?;
    for (table, count) in before {
        if table == "traffic_daily_snapshots" && old_version < 24 {
            // 0024 reconstructs daily projections from accepted raw samples.
            let invalid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM (SELECT subscription_id, substr(recorded_at, 1, 10) AS day, SUM(upload) AS up, SUM(download) AS down FROM subscription_traffic GROUP BY subscription_id, day) raw LEFT JOIN traffic_daily_snapshots daily ON daily.subscription_id = raw.subscription_id AND daily.date = raw.day WHERE daily.subscription_id IS NULL OR daily.total_upload <> raw.up OR daily.total_download <> raw.down)").fetch_one(pool).await?;
            if invalid || after.get(&table).is_none_or(|next| *next < count) {
                bail!("daily traffic projection backfill verification failed");
            }
            continue;
        }
        if table != "key_metadata" && after.get(&table) != Some(&count) {
            bail!("migration changed an existing business table row count");
        }
    }
    let impact = if let Some(plan) = plan {
        let key = key.context("original security recovery key required")?;
        let repository = SqliteNodePoolRepository::new_with_key(
            pool.clone(),
            Arc::new(MasterKey::from_bytes(
                key.as_bytes()
                    .try_into()
                    .context("invalid master-key length")?,
            )),
        );
        let before = repository.security_blocked_ids().await?;
        repository.apply_security_plan(plan).await?;
        let after = repository.security_blocked_ids().await?;
        Some(security::impact(plan, &before, &after))
    } else {
        None
    };
    if database::integrity_check(pool).await? != "ok" {
        bail!("staged migration integrity check failed");
    }
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await?;
    if !violations.is_empty() {
        bail!("staged migration foreign-key check failed");
    }
    Ok(impact)
}

async fn checkpoint(pool: &SqlitePool) -> Result<()> {
    let (busy, _, _): (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(pool)
        .await?;
    if busy != 0 {
        bail!("migration checkpoint is busy; no database replacement performed");
    }
    Ok(())
}

/// Lock before connecting: a waiter must open the newly published inode after
/// migration/restore, never retain a pre-rename SQLite connection.
pub(crate) async fn open_current_database(path: &str) -> Result<(DbLock, SqlitePool)> {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("management database must use its resolved regular path consistently");
    }
    let lock = DbLock::acquire_with_timeout(Path::new(path), Duration::from_secs(5))?;
    let pool = open_db(path, 1).await?;
    ensure_current_schema(&pool).await?;
    preflight::reject_unverified_key_state(&pool).await?;
    Ok((lock, pool))
}

/// A fresh bootstrap is allowed; an existing schema must be upgraded explicitly.
pub(crate) async fn ensure_current_schema(pool: &SqlitePool) -> Result<()> {
    let tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(pool)
    .await?;
    if tables == 0 {
        deve_sub_storage_sqlite::run_migrations(pool).await?;
    }
    deve_sub_storage_sqlite::verify_schema(pool).await.context(
        "existing database requires explicit `deve-sub migrate` before management commands",
    )?;
    deve_sub_storage_sqlite::verify_generation_cache_encryption(pool).await?;
    Ok(())
}
