//! `deve-sub serve` — start the HTTP server with all wiring.
//!
//! Extracted from `commands.rs` to keep that file under the 500-line fuse.
//! See `docs/plan/milestones/M1-infrastructure.md` for the server bootstrap
//! contract and `M4-sources-and-node-pool.md` Slice 4 for GeoIP and node
//! override wiring.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

use deve_sub_application::{
    DbHealthPort, GeoIpPort, GraceTokenCleanupScheduler, LoginRateLimiter, RefreshScheduler,
    SubscriptionFetcher, TrafficDailySnapshotScheduler,
};
use deve_sub_domain::{
    AuditLogRepository, GenerationCacheRepository, LatencyProbe, LatencyRecordRepository,
    NodeOverrideRepository, NodePoolRepository, PoolMetaRepository, ProbeRunRepository,
    ProbeSourceAdapter, ProbeSourceRepository, RecoveryCodeRepository, SessionRepository,
    ShortCodeRepository, SourceRepository, SourceSnapshotRepository, SubscriptionRepository,
    SubscriptionTokenRepository, TempLinkRepository, TemplateRepository, TemplateVersionRepository,
    TotpSecretRepository, TrafficDailySnapshotRepository, TrafficRepository, UserRepository,
};
use deve_sub_server::{AppState, build_router};

use crate::commands::{ServeArgs, ensure_db_dir, load_config, open_db};

/// Start the HTTP server.
pub async fn serve(args: ServeArgs) -> Result<()> {
    let mut config = load_config(&args.config)?;
    args.apply_overrides(&mut config);

    let bind: SocketAddr = config.server.bind.parse().context("invalid bind address")?;

    tracing::info!(
        product = %config.product_name,
        bind = %bind,
        headless = !config.server.serve_web,
        "starting server"
    );

    ensure_db_dir(&config.database.path)?;
    ensure_db_dir(&config.security.master_key_path)?;

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
    let pool_repo: Arc<dyn NodePoolRepository> = Arc::new(
        deve_sub_storage_sqlite::SqliteNodePoolRepository::new(db.clone()),
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
        deve_sub_storage_sqlite::SqliteProbeSourceRepository::new(db.clone()),
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

    let nezha_adapter: Arc<dyn ProbeSourceAdapter> = Arc::new(
        deve_sub_adapters::NezhaProbeAdapter::new(Arc::clone(&master_key)),
    );
    let dstatus_adapter: Arc<dyn ProbeSourceAdapter> = Arc::new(
        deve_sub_adapters::DStatusProbeAdapter::new(Arc::clone(&master_key)),
    );
    let komari_adapter: Arc<dyn ProbeSourceAdapter> = Arc::new(
        deve_sub_adapters::KomariProbeAdapter::new(Arc::clone(&master_key)),
    );
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
        Arc::new(deve_sub_storage_sqlite::SqliteHealthCheck::new(db));

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
        cancelled_flags: Arc::new(Mutex::new(HashMap::new())),
        fetcher: fetcher.clone(),
        geoip: geoip.clone(),
        rate_limiter,
        db_health,
    };

    let scheduler = RefreshScheduler::new(source_repo, snapshot_repo, pool_repo, fetcher, geoip);
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    // Crash recovery (constraint #20): mark any probe runs left in Running or
    // Pending as Failed before serving traffic.
    match deve_sub_application::probe::recover_crashed_runs(state.probe_run_repo.as_ref()).await {
        Ok(n) if n > 0 => tracing::info!(recovered = n, "marked crashed probe runs as failed"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "failed to recover crashed probe runs"),
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

    let traffic_snapshot_scheduler = TrafficDailySnapshotScheduler::new(
        Arc::clone(&state.traffic_repo),
        Arc::clone(&state.traffic_daily_snapshot_repo),
    );
    let traffic_snapshot_rx = shutdown_tx.subscribe();
    let traffic_snapshot_handle = tokio::spawn(async move {
        traffic_snapshot_scheduler
            .run(async move {
                let mut rx = traffic_snapshot_rx;
                let _ = rx.recv().await;
            })
            .await;
    });

    let router = build_router(state);

    let signal_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        create_shutdown_signal().await;
        let _ = signal_tx.send(());
    });

    let server_rx = shutdown_tx.subscribe();
    deve_sub_server::serve(router, bind, async move {
        let mut rx = server_rx;
        let _ = rx.recv().await;
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?;

    let _ = scheduler_handle.await;
    let _ = grace_handle.await;
    let _ = traffic_snapshot_handle.await;
    tracing::info!("refresh scheduler stopped, server exiting");

    Ok(())
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
