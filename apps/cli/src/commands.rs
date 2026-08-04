//! CLI subcommand implementations.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use deve_sub_application::{AppConfig, DbHealthPort, LoginRateLimiter};
use deve_sub_domain::{
    RecoveryCodeRepository, SessionRepository, TotpSecretRepository, UserRepository,
};
use deve_sub_server::{AppState, build_router};

/// Start the HTTP server.
#[derive(Args)]
pub struct ServeArgs {
    /// Path to configuration file.
    #[arg(long, env = "DEVE_SUB_CONFIG")]
    config: Option<PathBuf>,

    /// Bind address.
    #[arg(long, env = "DEVE_SUB_BIND")]
    bind: Option<String>,

    /// Run without web UI (API and subscription only).
    #[arg(long)]
    headless: bool,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH")]
    db_path: Option<String>,
}

impl ServeArgs {
    /// Apply CLI overrides to the loaded configuration in one place.
    fn apply_overrides(&self, config: &mut AppConfig) {
        if let Some(bind) = &self.bind {
            config.server.bind = bind.clone();
        }
        if self.headless {
            config.server.serve_web = false;
        }
        if let Some(db_path) = &self.db_path {
            config.database.path = db_path.clone();
        }
    }
}

/// Migrate command arguments.
#[derive(Args)]
pub struct MigrateArgs {
    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    db_path: String,
}

/// Doctor command arguments.
#[derive(Args)]
pub struct DoctorArgs {
    /// Path to configuration file.
    #[arg(long, env = "DEVE_SUB_CONFIG")]
    config: Option<PathBuf>,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH")]
    db_path: Option<String>,
}

/// Config validate command arguments.
#[derive(Args)]
pub struct ConfigValidateArgs {
    /// Path to configuration file.
    #[arg(long, env = "DEVE_SUB_CONFIG")]
    config: Option<PathBuf>,
}

/// OpenAPI export command arguments.
#[derive(Args)]
pub struct OpenapiArgs {
    /// Path to configuration file.
    #[arg(long, env = "DEVE_SUB_CONFIG")]
    config: Option<PathBuf>,
}

/// User management command container.
#[derive(Args)]
pub struct UserArgs {
    #[command(subcommand)]
    pub command: UserSubCommand,
}

/// User subcommands.
#[derive(Subcommand)]
pub enum UserSubCommand {
    /// Initialize the first admin user.
    InitAdmin(UserInitAdminArgs),
}

/// Arguments for `user init-admin`.
#[derive(Args)]
pub struct UserInitAdminArgs {
    /// Admin username.
    #[arg(long)]
    pub username: String,

    /// Admin password.
    #[arg(long)]
    pub password: String,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Config subcommand container.
#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigSubCommand,
}

/// Config subcommands.
#[derive(Subcommand)]
pub enum ConfigSubCommand {
    /// Validate configuration file.
    Validate(ConfigValidateArgs),
}

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
        deve_sub_security::MasterKey::load_or_generate(std::path::Path::new(
            &config.security.master_key_path,
        ))
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
        rate_limiter,
        db_health,
    };
    let router = build_router(state);

    let shutdown = create_shutdown_signal();

    deve_sub_server::serve(router, bind, shutdown)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    Ok(())
}

pub async fn doctor(args: DoctorArgs) -> Result<()> {
    let config = load_config(&args.config)?;
    let db_path = args.db_path.unwrap_or(config.database.path);

    println!("Deve Sub — System Diagnostics");
    println!("==============================");

    // Version check
    println!("\n[1/4] Version");
    println!("  deve-sub {}", env!("CARGO_PKG_VERSION"));

    // Database check
    println!("\n[2/4] Database");
    if Path::new(&db_path).exists() {
        println!("  database file: {db_path} (exists)");
    } else {
        println!("  database file: {db_path} (not found — run `deve-sub migrate` first)");
    }

    // Directories check
    println!("\n[3/4] Directories");
    let data_dir = Path::new(&db_path).parent().unwrap_or(Path::new("."));
    if data_dir.exists() {
        println!("  {}: exists", data_dir.display());
    } else {
        println!(
            "  {}: missing (will be created on first run)",
            data_dir.display()
        );
    }

    // Network check
    println!("\n[4/4] Network");
    println!("  network check: skipped (no external endpoints configured)");

    println!("\nDiagnostics complete.");
    Ok(())
}

pub async fn migrate(args: MigrateArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, "running migrations");

    ensure_db_dir(&args.db_path)?;

    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    println!("Migrations applied successfully to {db}", db = args.db_path);
    Ok(())
}

pub async fn config_validate(args: ConfigValidateArgs) -> Result<()> {
    let config = load_config(&args.config)?;
    println!("Configuration valid:");
    println!("  product_name: {}", config.product_name);
    println!("  server.bind:   {}", config.server.bind);
    println!("  server.serve_web: {}", config.server.serve_web);
    println!("  database.path: {}", config.database.path);
    Ok(())
}

pub async fn openapi(args: OpenapiArgs) -> Result<()> {
    let config = load_config(&args.config)?;
    let spec = deve_sub_server::routes::build_openapi_spec(&config.product_name);
    let json = serde_json::to_string_pretty(&spec).context("failed to serialize OpenAPI spec")?;
    println!("{json}");
    Ok(())
}

pub async fn user_init_admin(args: UserInitAdminArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, "initializing admin user");

    ensure_db_dir(&args.db_path)?;

    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let user_repo = deve_sub_storage_sqlite::SqliteUserRepository::new(pool);

    match deve_sub_application::auth::setup_admin(&user_repo, &args.username, &args.password).await
    {
        Ok(user) => {
            println!("Admin user created successfully:");
            println!("  id:       {}", user.id);
            println!("  username: {}", user.username);
            println!("  role:     {}", user.role);
            Ok(())
        }
        Err(deve_sub_application::AuthError::AlreadyInitialized) => {
            anyhow::bail!("admin user already exists — use the API or CLI to manage users");
        }
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

fn load_config(path: &Option<PathBuf>) -> Result<AppConfig> {
    match path {
        Some(p) => {
            let content = std::fs::read_to_string(p)
                .with_context(|| format!("failed to read config file: {}", p.display()))?;
            let config: AppConfig = serde_json::from_str(&content)
                .with_context(|| format!("failed to parse config file: {}", p.display()))?;
            Ok(config)
        }
        None => Ok(AppConfig::default()),
    }
}

/// Open a SQLite database pool with the given path and connection limit.
async fn open_db(path: &str, max_connections: u32) -> Result<sqlx::sqlite::SqlitePool> {
    let sqlite_config =
        deve_sub_storage_sqlite::SqliteConfig::new(path).max_connections(max_connections);
    deve_sub_storage_sqlite::create_pool(&sqlite_config)
        .await
        .context("failed to create database pool")
}

/// Create the parent directory of the database file if it does not exist.
fn ensure_db_dir(db_path: &str) -> Result<()> {
    if let Some(parent) = Path::new(db_path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).context("failed to create database directory")?;
    }
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
