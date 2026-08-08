//! `deve-sub serve` — start the HTTP server with all wiring.
//!
//! Extracted from `commands.rs` to keep that file under the 500-line fuse.
//! See `docs/plan/milestones/M1-infrastructure.md` for the server bootstrap
//! contract and `M4-sources-and-node-pool.md` Slice 4 for GeoIP and node
//! override wiring.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};

use deve_sub_application::{
    DbHealthPort, GeoIpPort, LoginRateLimiter, RefreshScheduler, SubscriptionFetcher,
};
use deve_sub_domain::{
    GenerationCacheRepository, NodeOverrideRepository, NodePoolRepository, PoolMetaRepository,
    RecoveryCodeRepository, SessionRepository, SourceRepository, SourceSnapshotRepository,
    SubscriptionRepository, SubscriptionTokenRepository, TemplateRepository,
    TemplateVersionRepository, TotpSecretRepository, UserRepository,
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
        deve_sub_storage_sqlite::SqliteSourceRepository::new(db.clone()),
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
        subscription_token_repo,
        fetcher: fetcher.clone(),
        geoip: geoip.clone(),
        rate_limiter,
        db_health,
    };

    let scheduler = RefreshScheduler::new(source_repo, snapshot_repo, pool_repo, fetcher, geoip);
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);
    let scheduler_rx = shutdown_tx.subscribe();
    let scheduler_handle = tokio::spawn(async move {
        scheduler
            .run(async move {
                let mut rx = scheduler_rx;
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
