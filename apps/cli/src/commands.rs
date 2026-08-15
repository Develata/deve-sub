//! CLI subcommand implementations.

use std::io::{self, BufRead as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};

use deve_sub_application::AppConfig;

/// Start the HTTP server.
#[derive(Args)]
pub struct ServeArgs {
    /// Path to configuration file.
    #[arg(long, env = "DEVE_SUB_CONFIG")]
    pub(crate) config: Option<PathBuf>,

    /// Bind address.
    #[arg(long, env = "DEVE_SUB_BIND")]
    pub(crate) bind: Option<String>,

    /// Run without web UI (API and subscription only).
    #[arg(long)]
    pub(crate) headless: bool,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH")]
    pub(crate) db_path: Option<String>,

    /// Path to the compiled web frontend dist directory.
    #[arg(long, env = "DEVE_SUB_WEB_DIST_DIR")]
    pub(crate) web_dist_dir: Option<String>,
}

impl ServeArgs {
    /// Apply CLI overrides to the loaded configuration in one place.
    pub(crate) fn apply_overrides(&self, config: &mut AppConfig) {
        if let Some(bind) = &self.bind {
            config.server.bind = bind.clone();
        }
        if self.headless {
            config.server.serve_web = false;
        }
        if let Some(db_path) = &self.db_path {
            config.database.path = db_path.clone();
        }
        if let Some(web_dist_dir) = &self.web_dist_dir {
            config.server.web_dist_dir = web_dist_dir.clone();
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
///
/// DS-AUD-028: `--password` on the argv is visible in the process list
/// (`ps aux`). Prefer `--password-stdin` (reads from stdin) or
/// `--password-env VAR` (reads from an environment variable) for
/// production use. `--password` is retained for backward compatibility
/// but emits a warning.
#[derive(Args)]
pub struct UserInitAdminArgs {
    /// Admin username.
    #[arg(long)]
    pub username: String,

    /// Admin password. Visible in the process list — prefer
    /// `--password-stdin` or `--password-env` for production.
    #[arg(long)]
    pub password: Option<String>,

    /// Read the admin password from stdin (one line, trailing newline
    /// stripped). Avoids exposing the password in the process list.
    #[arg(long)]
    pub password_stdin: bool,

    /// Read the admin password from the named environment variable.
    /// Avoids exposing the password in the process list.
    #[arg(long, value_name = "VAR")]
    pub password_env: Option<String>,

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

/// Source management command container.
#[derive(Args)]
pub struct SourceArgs {
    #[command(subcommand)]
    pub command: SourceSubCommand,
}

/// Source subcommands.
#[derive(Subcommand)]
pub enum SourceSubCommand {
    /// Add a new subscription source.
    Add(SourceAddArgs),
}

/// Arguments for `source add`.
#[derive(Args)]
pub struct SourceAddArgs {
    /// Human-readable source name.
    #[arg(long)]
    pub name: String,

    /// Input format. One of: auto, base64, uri_list, mihomo_yaml, singbox_json,
    /// xray_json, v2ray_json, shadowrocket.
    #[arg(long, default_value = "auto")]
    pub source_type: String,

    /// Subscription URL.
    #[arg(long)]
    pub url: String,

    /// Enable automatic refresh.
    #[arg(long)]
    pub auto_update: bool,

    /// Refresh interval in seconds.
    #[arg(long, default_value = "3600")]
    pub update_interval_secs: u64,

    /// Keep existing nodes if a refresh fails.
    #[arg(long, default_value = "true")]
    pub keep_on_fail: bool,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,

    /// Master key file path.
    #[arg(long, env = "DEVE_SUB_KEY_PATH", default_value = "data/master.key")]
    pub key_path: String,
}

pub use crate::node_cmds::{NodeArgs, NodeSubCommand, node_import, node_list};
pub use crate::template_cmds::{
    TemplateArgs, TemplateSubCommand, template_add, template_delete, template_get, template_list,
    template_rollback, template_update, template_versions,
};

pub async fn doctor(args: DoctorArgs) -> Result<()> {
    let config = load_config(&args.config)?;
    let db_path = args.db_path.unwrap_or_else(|| config.database.path.clone());

    println!("Deve Sub — System Diagnostics");
    println!("==============================");

    // Version check
    println!("\n[1/4] Version");
    println!("  deve-sub {}", env!("CARGO_PKG_VERSION"));

    // Database check
    println!("\n[2/4] Database");
    if Path::new(&db_path).exists() {
        println!("  database file: {db_path} (exists)");
        match check_database(&db_path).await {
            Ok(schema_ver) => {
                println!("  schema version: {schema_ver}");
            }
            Err(e) => {
                println!("  WARNING: failed to open database: {e}");
            }
        }
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
    let bind = &config.server.bind;
    match check_bind_available(bind).await {
        Ok(()) => println!("  bind {bind}: available"),
        Err(e) => println!("  bind {bind}: WARNING — {e}"),
    }

    println!("\nDiagnostics complete.");
    Ok(())
}

async fn check_database(db_path: &str) -> Result<i64> {
    let pool = open_db(db_path, 1).await?;
    let row: (i64,) = sqlx::query_as("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .context("failed to query schema version")?;
    Ok(row.0)
}

async fn check_bind_available(bind_addr: &str) -> Result<()> {
    let addr: std::net::SocketAddr = bind_addr
        .parse()
        .context("invalid bind address (expected host:port)")?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot bind to {bind_addr} (already in use?)"))?;
    drop(listener);
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

    let password = resolve_admin_password(&args)?;

    ensure_db_dir(&args.db_path)?;

    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let user_repo = deve_sub_storage_sqlite::SqliteUserRepository::new(pool);

    match deve_sub_application::auth::setup_admin(&user_repo, &args.username, &password).await {
        Ok(user) => {
            println!("Admin user created successfully:");
            println!("  id:       {}", user.id);
            println!("  username: {}", user.username);
            println!("  role:     {}", user.role);
            Ok(())
        }
        Err(deve_sub_application::AuthError::AlreadyInitialized) => {
            bail!("admin user already exists — use the API or CLI to manage users");
        }
        Err(e) => Err(anyhow!(e)),
    }
}

/// DS-AUD-028: keeps the admin password out of the process list by
/// preferring stdin/env over `--password`.
fn resolve_admin_password(args: &UserInitAdminArgs) -> Result<String> {
    resolve_admin_password_with(args, |name: &str| std::env::var(name), read_stdin_line)
}

fn read_stdin_line() -> Result<String> {
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .context("failed to read password from stdin")?;
    Ok(line)
}

fn resolve_admin_password_with<E, S>(
    args: &UserInitAdminArgs,
    env_lookup: E,
    stdin_read: S,
) -> Result<String>
where
    E: Fn(&str) -> Result<String, std::env::VarError>,
    S: FnOnce() -> Result<String>,
{
    if args.password_stdin {
        let line = stdin_read()?;
        let pw = line.trim_end_matches(['\r', '\n']).to_string();
        if pw.is_empty() {
            bail!("no password read from stdin (stdin was empty)");
        }
        return Ok(pw);
    }

    if let Some(var) = &args.password_env {
        return env_lookup(var)
            .with_context(|| format!("failed to read password from env var `{var}`"));
    }

    if let Some(pw) = &args.password {
        eprintln!(
            "warning: --password is visible in the process list; prefer --password-stdin or --password-env"
        );
        return Ok(pw.clone());
    }

    Err(anyhow!(
        "no password source provided; use one of --password, --password-stdin, or --password-env"
    ))
}

pub async fn source_add(args: SourceAddArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, name = %args.name, "adding source");

    ensure_db_dir(&args.db_path)?;

    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let master_key = Arc::new(
        deve_sub_security::MasterKey::load_or_generate(std::path::Path::new(&args.key_path))
            .context("failed to load master key")?,
    );
    let source_repo = deve_sub_storage_sqlite::SqliteSourceRepository::new_with_key(
        pool,
        Arc::clone(&master_key),
    );

    let source_type = args
        .source_type
        .parse::<deve_sub_domain::SourceType>()
        .map_err(|e| anyhow::anyhow!("invalid --source-type: {e}"))?;

    let params = deve_sub_application::source::CreateSourceParams {
        name: args.name.clone(),
        source_type,
        url: args.url.clone(),
        auto_update: args.auto_update,
        update_interval_secs: args.update_interval_secs,
        keep_on_fail: args.keep_on_fail,
        filter_rules: None,
    };

    match deve_sub_application::source::create_source(&source_repo, params).await {
        Ok(source) => {
            println!("Source created successfully:");
            println!("  id:                {}", source.id);
            println!("  name:              {}", source.name);
            println!("  source_type:       {}", source.source_type);
            println!(
                "  url:               {}",
                deve_sub_security::mask_url(&source.url)
            );
            println!("  auto_update:       {}", source.auto_update);
            println!("  update_interval:   {}s", source.update_interval_secs);
            println!("  enabled:           {}", source.enabled);
            println!("  keep_on_fail:      {}", source.keep_on_fail);
            Ok(())
        }
        Err(deve_sub_application::source::SourceAppError::InvalidInput(msg)) => {
            anyhow::bail!("invalid input: {msg}");
        }
        Err(deve_sub_application::source::SourceAppError::NameExists) => {
            anyhow::bail!("source name '{}' already exists", args.name);
        }
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

pub(crate) fn load_config(path: &Option<PathBuf>) -> Result<AppConfig> {
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
pub(crate) async fn open_db(path: &str, max_connections: u32) -> Result<sqlx::sqlite::SqlitePool> {
    let sqlite_config =
        deve_sub_storage_sqlite::SqliteConfig::new(path).max_connections(max_connections);
    deve_sub_storage_sqlite::create_pool(&sqlite_config)
        .await
        .context("failed to create database pool")
}

/// Create the parent directory of the database file if it does not exist.
pub(crate) fn ensure_db_dir(db_path: &str) -> Result<()> {
    if let Some(parent) = Path::new(db_path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).context("failed to create database directory")?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    const NO_ENV: fn(&str) -> Result<String, std::env::VarError> =
        |_| Err(std::env::VarError::NotPresent);
    const EOF_STDIN: fn() -> Result<String> = || Ok(String::new());

    /// DS-AUD-028: `--password-env` reads the password from the named
    /// environment variable, keeping it out of the process list.
    #[test]
    fn password_env_reads_from_environment() {
        let args = UserInitAdminArgs {
            username: "admin".into(),
            password: None,
            password_stdin: false,
            password_env: Some("DEVE_SUB_TEST_ADMIN_PW_ENV".into()),
            db_path: "data/deve-sub.db".into(),
        };
        let lookup = |name: &str| match name {
            "DEVE_SUB_TEST_ADMIN_PW_ENV" => Ok("s3cret-from-env".to_string()),
            _ => Err(std::env::VarError::NotPresent),
        };

        let resolved = resolve_admin_password_with(&args, lookup, EOF_STDIN).unwrap();
        assert_eq!(resolved, "s3cret-from-env");
    }

    /// DS-AUD-028: when the named env var is missing, the CLI errors
    /// instead of falling back to an empty password.
    #[test]
    fn password_env_missing_errors() {
        let var_name = "DEVE_SUB_TEST_ADMIN_PW_MISSING";
        let args = UserInitAdminArgs {
            username: "admin".into(),
            password: None,
            password_stdin: false,
            password_env: Some(var_name.into()),
            db_path: "data/deve-sub.db".into(),
        };

        let err = resolve_admin_password_with(&args, NO_ENV, EOF_STDIN).unwrap_err();
        assert!(
            err.to_string().contains(var_name),
            "error should name the missing env var: {err}"
        );
    }

    /// DS-AUD-028: when no password source is provided, the CLI errors
    /// rather than silently using an empty password.
    #[test]
    fn password_no_source_errors() {
        let args = UserInitAdminArgs {
            username: "admin".into(),
            password: None,
            password_stdin: false,
            password_env: None,
            db_path: "data/deve-sub.db".into(),
        };

        let err = resolve_admin_password_with(&args, NO_ENV, EOF_STDIN).unwrap_err();
        assert!(
            err.to_string().contains("no password source provided"),
            "error should explain the missing source: {err}"
        );
    }

    /// DS-AUD-028: the legacy `--password` argv form still works for
    /// backward compatibility (with a warning to stderr).
    #[test]
    fn password_argv_legacy_works() {
        let args = UserInitAdminArgs {
            username: "admin".into(),
            password: Some("legacy-pw".into()),
            password_stdin: false,
            password_env: None,
            db_path: "data/deve-sub.db".into(),
        };

        let resolved = resolve_admin_password_with(&args, NO_ENV, EOF_STDIN).unwrap();
        assert_eq!(resolved, "legacy-pw");
    }

    /// DS-AUD-028: `--password-env` takes priority over `--password`.
    #[test]
    fn password_env_takes_priority_over_argv() {
        let args = UserInitAdminArgs {
            username: "admin".into(),
            password: Some("argv-value".into()),
            password_stdin: false,
            password_env: Some("DEVE_SUB_TEST_ADMIN_PW_PRIORITY".into()),
            db_path: "data/deve-sub.db".into(),
        };
        let lookup = |name: &str| match name {
            "DEVE_SUB_TEST_ADMIN_PW_PRIORITY" => Ok("env-value".to_string()),
            _ => Err(std::env::VarError::NotPresent),
        };

        let resolved = resolve_admin_password_with(&args, lookup, EOF_STDIN).unwrap();
        assert_eq!(resolved, "env-value");
    }

    /// DS-AUD-028: `--password-stdin` reads one line and strips the
    /// trailing newline.
    #[test]
    fn password_stdin_reads_line() {
        let args = UserInitAdminArgs {
            username: "admin".into(),
            password: None,
            password_stdin: true,
            password_env: None,
            db_path: "data/deve-sub.db".into(),
        };
        let stdin = || Ok("stdin-pw\n".to_string());

        let resolved = resolve_admin_password_with(&args, NO_ENV, stdin).unwrap();
        assert_eq!(resolved, "stdin-pw");
    }

    /// DS-AUD-028: `--password-stdin` takes priority over `--password-env`
    /// and `--password`.
    #[test]
    fn password_stdin_takes_priority() {
        let args = UserInitAdminArgs {
            username: "admin".into(),
            password: Some("argv-value".into()),
            password_stdin: true,
            password_env: Some("DEVE_SUB_TEST_ADMIN_PW_STDIN".into()),
            db_path: "data/deve-sub.db".into(),
        };
        let lookup = |name: &str| match name {
            "DEVE_SUB_TEST_ADMIN_PW_STDIN" => Ok("env-value".to_string()),
            _ => Err(std::env::VarError::NotPresent),
        };
        let stdin = || Ok("stdin-wins\n".to_string());

        let resolved = resolve_admin_password_with(&args, lookup, stdin).unwrap();
        assert_eq!(resolved, "stdin-wins");
    }

    /// DS-AUD-028: empty stdin (EOF) must error, not silently yield an
    /// empty password.
    #[test]
    fn password_stdin_empty_errors() {
        let args = UserInitAdminArgs {
            username: "admin".into(),
            password: None,
            password_stdin: true,
            password_env: None,
            db_path: "data/deve-sub.db".into(),
        };

        let err = resolve_admin_password_with(&args, NO_ENV, EOF_STDIN).unwrap_err();
        assert!(
            err.to_string().contains("stdin was empty"),
            "error should explain empty stdin: {err}"
        );
    }
}
