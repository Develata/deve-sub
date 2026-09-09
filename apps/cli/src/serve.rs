//! `deve-sub serve` — start the HTTP server with all wiring.
//!
//! Extracted from `commands.rs` to keep that file under the 500-line fuse.
//! See `docs/plan/milestones/M1-infrastructure.md` for the server bootstrap
//! contract and `M4-sources-and-node-pool.md` Slice 4 for GeoIP and node
//! override wiring.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};

use deve_sub_application::{
    DbHealthPort, GeoIpPort, GraceTokenCleanupScheduler, JobSupervisor, LoginRateLimiter,
    RefreshScheduler, SubscriptionFetcher,
};
use deve_sub_domain::{
    AuditLogRepository, GenerationCacheRepository, LatencyProbe, LatencyRecordRepository,
    NodeOverrideRepository, NodePoolRepository, PoolMetaRepository, ProbeRunRepository,
    ProbeSourceAdapter, ProbeSourceRepository, RecoveryCodeRepository, SessionRepository,
    ShortCodeRepository, SourceRefreshJobRepository, SourceRepository, SourceSnapshotRepository,
    SubscriptionRepository, SubscriptionTokenRepository, TempLinkRepository, TemplateRepository,
    TemplateVersionRepository, TotpSecretRepository, TrafficDailySnapshotRepository,
    TrafficRepository, UserRepository,
};
use deve_sub_server::{AppState, build_router};

use crate::commands::{ServeArgs, ensure_db_dir, load_config, open_db};
use crate::db_lock::DbLock;

/// Start the HTTP server.
pub async fn serve(args: ServeArgs) -> Result<()> {
    let mut config = load_config(&args.config)?;
    args.apply_overrides(&mut config);

    // P0-06: validate config before anything else. Errors are fatal;
    // Warnings are logged but do not block startup.
    let issues = config.validate();
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == deve_sub_application::IssueSeverity::Error)
        .collect();
    for issue in &issues {
        match issue.severity {
            deve_sub_application::IssueSeverity::Error => {
                tracing::error!(issue = %issue.message, "config validation error");
            }
            deve_sub_application::IssueSeverity::Warning => {
                tracing::warn!(issue = %issue.message, "config validation warning");
            }
        }
    }
    if !errors.is_empty() {
        anyhow::bail!(
            "config validation failed with {} error(s); run `deve-sub config validate` for details",
            errors.len()
        );
    }

    let bind: SocketAddr = config.server.bind.parse().context("invalid bind address")?;

    tracing::info!(
        product = %config.product_name,
        bind = %bind,
        headless = !config.server.serve_web,
        "starting server"
    );

    ensure_db_dir(&config.database.path)?;
    ensure_db_dir(&config.security.master_key_path)?;

    // WHY hold an exclusive flock for the entire process lifetime: SQLite's
    // own locking detects concurrent write transactions but does not prevent
    // two `serve` processes from opening pools on the same DB file. The
    // flock is the authoritative process-level guard (ADR-0007 §7).
    // `_db_lock` is dropped on function exit, releasing the lock.
    //
    // DS-AUD-B05: bounded 5s timeout (not blocking-forever) and the lock
    // is held on a sidecar file, so a `restore` rename of the DB inode
    // cannot free the lock out from under a running `serve`.
    let _db_lock = DbLock::acquire_with_timeout(
        std::path::Path::new(&config.database.path),
        Duration::from_secs(5),
    )?;

    let db = open_db(&config.database.path, 8).await?;
    deve_sub_storage_sqlite::verify_schema(&db)
        .await
        .context("database schema check failed — run `deve-sub migrate` first")?;

    let master_key = Arc::new(
        if config.security.allow_master_key_generation {
            deve_sub_security::MasterKey::load_or_generate(std::path::Path::new(
                &config.security.master_key_path,
            ))
        } else {
            deve_sub_security::MasterKey::load(std::path::Path::new(
                &config.security.master_key_path,
            ))
        }
        .context("failed to load master key")?,
    );
    // WHY (DS-AUD-B07): bind the DB to the loaded key, or verify the key
    // matches the one already bound. Fail-closed on mismatch prevents serve
    // from starting with the wrong key (which would make old ciphertext
    // unreadable). When `allow_master_key_generation=true`, a fresh key may
    // be generated for an empty DB — the binding records it as the owner.
    {
        let fp = master_key
            .fingerprint()
            .context("failed to compute master key fingerprint")?;
        deve_sub_storage_sqlite::ensure_key_binding(&db, &fp).await?;
    }

    let user_repo: Arc<dyn UserRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteUserRepository::new(db.clone()),
    );
    let audit_log_repo: Arc<dyn AuditLogRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteAuditLogRepository::new(db.clone()),
    );
    let session_repo: Arc<dyn SessionRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteSessionRepository::new(db.clone()),
    );
    let totp_secret_repo: Arc<dyn TotpSecretRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteTotpSecretRepository::new(db.clone()),
    );
    let recovery_code_repo: Arc<dyn RecoveryCodeRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteRecoveryCodeRepository::new(db.clone()),
    );
    let source_repo: Arc<dyn SourceRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteSourceRepository::new_with_key(
            db.clone(),
            Arc::clone(&master_key),
        ),
    );
    let snapshot_repo: Arc<dyn SourceSnapshotRepository> =
        Arc::new(deve_sub_storage_sqlite::SqliteSourceSnapshotRepository::new(db.clone()));
    let refresh_job_repo: Arc<dyn SourceRefreshJobRepository> =
        Arc::new(deve_sub_storage_sqlite::SqliteSourceRefreshJobRepository::new(db.clone()));
    let pool_repo: Arc<dyn NodePoolRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteNodePoolRepository::new_with_key(
            db.clone(),
            Arc::clone(&master_key),
        ),
    );
    let override_repo: Arc<dyn NodeOverrideRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteNodeOverrideRepository::new(db.clone()),
    );
    let template_repo: Arc<dyn TemplateRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteTemplateRepository::new(db.clone()),
    );
    let version_repo: Arc<dyn TemplateVersionRepository> =
        Arc::new(deve_sub_storage_sqlite::SqliteTemplateVersionRepository::new(db.clone()));
    let pool_meta_repo: Arc<dyn PoolMetaRepository> = Arc::new(
        deve_sub_storage_sqlite::SqlitePoolMetaRepository::new(db.clone()),
    );
    let cache_repo: Arc<dyn GenerationCacheRepository> =
        Arc::new(deve_sub_storage_sqlite::SqliteGenerationCacheRepository::new(db.clone()));
    let subscription_repo: Arc<dyn SubscriptionRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteSubscriptionRepository::new(db.clone()),
    );
    let subscription_token_repo: Arc<dyn SubscriptionTokenRepository> =
        Arc::new(deve_sub_storage_sqlite::SqliteSubscriptionTokenRepository::new(db.clone()));
    let short_code_repo: Arc<dyn ShortCodeRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteShortCodeRepository::new(db.clone()),
    );
    let temp_link_repo: Arc<dyn TempLinkRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteTempLinkRepository::new(db.clone()),
    );
    let traffic_repo: Arc<dyn TrafficRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteTrafficRepository::new(db.clone()),
    );
    let traffic_daily_snapshot_repo: Arc<dyn TrafficDailySnapshotRepository> =
        Arc::new(deve_sub_storage_sqlite::SqliteTrafficDailySnapshotRepository::new(db.clone()));
    let probe_source_repo: Arc<dyn ProbeSourceRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteProbeSourceRepository::new_with_key(
            db.clone(),
            Arc::clone(&master_key),
        ),
    );
    let probe_run_repo: Arc<dyn ProbeRunRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteProbeRunRepository::new(db.clone()),
    );
    let latency_repo: Arc<dyn LatencyRecordRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteLatencyRecordRepository::new(db.clone()),
    );
    let tcp_probe: Arc<dyn LatencyProbe> = Arc::new(deve_sub_adapters::TcpConnectProbe::new());
    let quic_probe: Arc<dyn LatencyProbe> = Arc::new(deve_sub_adapters::QuicHandshakeProbe::new());
    let real_proxy_probe: Arc<dyn LatencyProbe> =
        Arc::new(deve_sub_adapters::RealProxyProbe::new());

    let nezha_adapter: Arc<dyn ProbeSourceAdapter> =
        Arc::new(deve_sub_adapters::NezhaProbeAdapter::new());
    let dstatus_adapter: Arc<dyn ProbeSourceAdapter> =
        Arc::new(deve_sub_adapters::DStatusProbeAdapter::new());
    let komari_adapter: Arc<dyn ProbeSourceAdapter> =
        Arc::new(deve_sub_adapters::KomariProbeAdapter::new());
    let probe_adapter: Arc<dyn ProbeSourceAdapter> = Arc::new(
        deve_sub_adapters::ProbeSourceAdapterRegistry::new()
            .with_nezha(nezha_adapter)
            .with_dstatus(dstatus_adapter)
            .with_komari(komari_adapter),
    );

    let fetcher: Arc<dyn SubscriptionFetcher> = Arc::new(deve_sub_adapters::HttpFetcher::new());
    let geoip: Arc<dyn GeoIpPort> = Arc::new(deve_sub_adapters::MaxMindGeoIp::new(
        config.geoip.mmdb_path.as_deref(),
    ));

    let rate_limiter: Arc<dyn LoginRateLimiter> =
        Arc::new(deve_sub_inmemory::InMemoryLoginRateLimiter::new(
            config.security.max_login_attempts,
            std::time::Duration::from_secs(config.security.lockout_duration_secs),
        ));

    let db_health: Arc<dyn DbHealthPort> =
        Arc::new(deve_sub_storage_sqlite::SqliteHealthCheck::new(db.clone()));

    let cancelled_flags: Arc<
        Mutex<HashMap<deve_sub_kernel::ProbeRunId, Arc<std::sync::atomic::AtomicBool>>>,
    > = Arc::new(Mutex::new(HashMap::new()));
    let refresh_cancel_flags: Arc<
        Mutex<HashMap<deve_sub_kernel::SourceRefreshJobId, Arc<std::sync::atomic::AtomicBool>>>,
    > = Arc::new(Mutex::new(HashMap::new()));
    let job_supervisor = Arc::new(JobSupervisor::new());

    let state = AppState {
        config: config.clone(),
        master_key,
        audit_log_repo,
        user_repo,
        session_repo,
        totp_secret_repo,
        recovery_code_repo,
        source_repo: source_repo.clone(),
        snapshot_repo: snapshot_repo.clone(),
        refresh_job_repo: refresh_job_repo.clone(),
        pool_repo: pool_repo.clone(),
        pool_meta_repo,
        override_repo,
        template_repo,
        version_repo,
        cache_repo,
        subscription_repo,
        subscription_token_repo: subscription_token_repo.clone(),
        short_code_repo,
        temp_link_repo,
        traffic_repo,
        traffic_daily_snapshot_repo,
        probe_source_repo,
        probe_run_repo,
        latency_repo,
        probe_adapter,
        tcp_probe,
        quic_probe,
        real_proxy_probe,
        cancelled_flags: Arc::clone(&cancelled_flags),
        refresh_cancel_flags: Arc::clone(&refresh_cancel_flags),
        job_supervisor: Arc::clone(&job_supervisor),
        fetcher: fetcher.clone(),
        geoip: geoip.clone(),
        rate_limiter: Arc::clone(&rate_limiter),
        db_health,
    };

    let scheduler = RefreshScheduler::new(
        source_repo,
        snapshot_repo,
        pool_repo,
        refresh_job_repo,
        fetcher,
        geoip,
    );
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    // Crash recovery (constraint #20): mark any probe runs left in Running or
    // Pending as Failed before serving traffic.
    match deve_sub_application::probe::recover_crashed_runs(state.probe_run_repo.as_ref()).await {
        Ok(n) if n > 0 => tracing::info!(recovered = n, "marked crashed probe runs as failed"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "failed to recover crashed probe runs"),
    }

    // Crash recovery (constraint #20): mark any source refresh jobs left in
    // Pending or Running as Failed. A stuck Running job holds the per-source
    // lease (partial UNIQUE index) and blocks all future refreshes for that
    // source (P0-10).
    match deve_sub_application::recover_crashed_refresh_jobs(state.refresh_job_repo.as_ref()).await
    {
        Ok(n) if n > 0 => tracing::info!(
            recovered = n,
            "marked crashed source refresh jobs as failed"
        ),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "failed to recover crashed source refresh jobs"),
    }

    let scheduler_rx = shutdown_tx.subscribe();
    let scheduler_handle = tokio::spawn(async move {
        scheduler
            .run(async move {
                let mut rx = scheduler_rx;
                let _ = rx.recv().await;
            })
            .await;
    });

    let grace_scheduler = GraceTokenCleanupScheduler::new(subscription_token_repo);
    let grace_rx = shutdown_tx.subscribe();
    let grace_handle = tokio::spawn(async move {
        grace_scheduler
            .run(async move {
                let mut rx = grace_rx;
                let _ = rx.recv().await;
            })
            .await;
    });

    let maintenance = Arc::new(deve_sub_storage_sqlite::SqliteMaintenance::new(
        db.clone(),
        &config.database.path,
    ));
    let maintenance_handle = tokio::spawn(crate::runtime::maintain(
        Arc::clone(&maintenance),
        Arc::clone(&job_supervisor),
        Arc::clone(&rate_limiter),
        shutdown_tx.subscribe(),
    ));
    let router = build_router(state);

    let signal_tx = shutdown_tx.clone();
    let signal_handle = tokio::spawn(async move {
        create_shutdown_signal().await;
        let _ = signal_tx.send(());
    });

    let server_rx = shutdown_tx.subscribe();
    let closing_jobs = Arc::clone(&job_supervisor);
    let server_result = deve_sub_server::serve(router, bind, async move {
        let mut rx = server_rx;
        let _ = rx.recv().await;
        closing_jobs.close();
    })
    .await;
    // Bind errors also own the shutdown path; do not detach initialized workers.
    let _ = shutdown_tx.send(());
    job_supervisor.close();
    signal_handle.abort();
    let _ = signal_handle.await;

    for flags in [
        cancelled_flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>(),
        refresh_cancel_flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>(),
    ] {
        for flag in flags {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    stop_scheduler(scheduler_handle, "refresh-scheduler").await;
    stop_scheduler(grace_handle, "grace-token-scheduler").await;

    stop_scheduler(maintenance_handle, "sqlite-maintenance").await;
    job_supervisor.shutdown(Duration::from_secs(30)).await;
    crate::runtime::checkpoint(&maintenance).await;
    if tokio::time::timeout(Duration::from_secs(10), db.close())
        .await
        .is_err()
    {
        tracing::warn!("database pool close timed out");
    }
    tracing::info!(
        tracked_jobs = job_supervisor.len(),
        task_panics = job_supervisor.panic_count(),
        task_cancellations = job_supervisor.cancellation_count(),
        rate_limiter_entries = rate_limiter.resident_entries(),
        "background jobs stopped, server exiting"
    );
    server_result.map_err(|e| anyhow::anyhow!(e))?;

    Ok(())
}

/// Await a scheduler task with a bounded grace period (constraint #20).
///
/// WHY: `RefreshScheduler::run` only observes the shutdown signal between
/// ticks; a tick with slow in-flight fetches (up to the HTTP timeout each)
/// would otherwise hold process exit open indefinitely. After the grace
/// period the task is aborted — refresh jobs are lease-tracked in the DB and
/// recovered as Failed on next start, so aborting mid-tick loses nothing.
async fn stop_scheduler(mut handle: tokio::task::JoinHandle<()>, name: &'static str) {
    match tokio::time::timeout(Duration::from_secs(30), &mut handle).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(task = name, error = %e, "scheduler task panicked"),
        Err(_) => {
            tracing::warn!(
                task = name,
                "scheduler exceeded 30s shutdown grace — aborting"
            );
            handle.abort();
            match tokio::time::timeout(Duration::from_secs(1), &mut handle).await {
                Ok(Err(error)) if !error.is_cancelled() => {
                    tracing::warn!(task = name, %error, "scheduler join failed")
                }
                Err(_) => tracing::warn!(task = name, "scheduler did not yield after abort"),
                _ => {}
            }
        }
    }
}

/// Create a shutdown future that listens for SIGTERM and SIGINT.
async fn create_shutdown_signal() {
    let sigterm = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::warn!("failed to install SIGTERM handler: {e}");
                std::future::pending::<()>().await;
            }
        }
    };

    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!("failed to listen for ctrl_c: {e}");
            std::future::pending::<()>().await;
        }
    };

    tokio::select! {
        _ = sigterm => {}
        _ = ctrl_c => {}
    }

    tracing::info!("shutdown signal received, draining connections");
}
