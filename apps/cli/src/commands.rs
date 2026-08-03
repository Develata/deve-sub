//! CLI subcommand implementations.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use deve_sub_application::AppConfig;
use deve_sub_server::{AppState, build_router};

/// Start the HTTP server.
#[derive(Args)]
pub struct ServeArgs {
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

/// Migrate command arguments.
#[derive(Args)]
pub struct MigrateArgs {
    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    db_path: String,
}

/// Config validate command arguments.
#[derive(Args)]
pub struct ConfigValidateArgs {
    /// Path to configuration file.
    #[arg(long, env = "DEVE_SUB_CONFIG")]
    config: Option<PathBuf>,
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
    let mut config = load_config(&args.config_path())?;
    if let Some(bind) = &args.bind {
        config.server.bind = bind.clone();
    }
    if args.headless {
        config.server.serve_web = false;
    }
    if let Some(db_path) = &args.db_path {
        config.database.path = db_path.clone();
    }

    let bind: SocketAddr = config.server.bind.parse().context("invalid bind address")?;

    tracing::info!(
        product = %config.product_name,
        bind = %bind,
        headless = !config.server.serve_web,
        "starting server"
    );

    let sqlite_config = deve_sub_storage_sqlite::SqliteConfig::new(&config.database.path);
    let db = deve_sub_storage_sqlite::create_pool(&sqlite_config)
        .await
        .context("failed to create database pool")?;

    let state = AppState {
        config: config.clone(),
        db,
    };
    let router = build_router(state);

    deve_sub_server::serve(router, bind, tokio::signal::unix::SignalKind::terminate())
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    Ok(())
}

pub async fn doctor() -> Result<()> {
    println!("Deve Sub — System Diagnostics");
    println!("==============================");

    // Version check
    println!("\n[1/4] Version");
    println!("  deve-sub {}", env!("CARGO_PKG_VERSION"));

    // Database check
    println!("\n[2/4] Database");
    let db_path = "data/deve-sub.db";
    if Path::new(db_path).exists() {
        println!("  database file: {db_path} (exists)");
    } else {
        println!("  database file: {db_path} (not found — run `deve-sub migrate` first)");
    }

    // Directories check
    println!("\n[3/4] Directories");
    let data_dir = Path::new("data");
    if data_dir.exists() {
        println!("  data/: exists");
    } else {
        println!("  data/: missing (will be created on first run)");
    }

    // Network check
    println!("\n[4/4] Network");
    println!("  network check: skipped (no external endpoints configured)");

    println!("\nDiagnostics complete.");
    Ok(())
}

pub async fn migrate(args: MigrateArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, "running migrations");

    if let Some(parent) = Path::new(&args.db_path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).context("failed to create database directory")?;
    }

    let sqlite_config =
        deve_sub_storage_sqlite::SqliteConfig::new(&args.db_path).max_connections(1);
    let pool = deve_sub_storage_sqlite::create_pool(&sqlite_config)
        .await
        .context("failed to connect to database")?;

    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .context("failed to run migrations")?;

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

impl ServeArgs {
    fn config_path(&self) -> Option<PathBuf> {
        std::env::var("DEVE_SUB_CONFIG").ok().map(PathBuf::from)
    }
}
