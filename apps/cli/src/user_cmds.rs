//! Administrator CLI initialization and password-source handling.

use std::io::{self, BufRead as _};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};

use crate::commands::{ensure_db_dir, open_db};

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

    /// Skip initialization without modifying existing users or reading credentials.
    #[arg(long)]
    pub if_needed: bool,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    pub db_path: String,
}

/// Create the first administrator using the shared application command.
pub async fn user_init_admin(args: UserInitAdminArgs) -> Result<()> {
    tracing::info!(db_path = %args.db_path, "initializing admin user");

    // Preserve eager credential errors for the manual command. Bootstrap must
    // check existing users before touching a stale or missing password source.
    let password = if args.if_needed {
        None
    } else {
        Some(resolve_admin_password(&args)?)
    };

    ensure_db_dir(&args.db_path)?;

    let pool = open_db(&args.db_path, 1).await?;
    deve_sub_storage_sqlite::run_migrations(&pool).await?;

    let user_repo = deve_sub_storage_sqlite::SqliteUserRepository::new(pool);

    if args.if_needed && deve_sub_application::auth::is_initialized(&user_repo).await? {
        println!("Admin initialization skipped: existing users are unchanged.");
        return Ok(());
    }
    let password = match password {
        Some(password) => password,
        None => resolve_admin_password(&args)?,
    };

    match deve_sub_application::auth::setup_admin(&user_repo, &args.username, &password).await {
        Ok(user) => {
            if args.if_needed {
                println!("Admin user initialized successfully.");
                return Ok(());
            }
            println!("Admin user created successfully:");
            println!("  id:       {}", user.id);
            println!("  username: {}", user.username);
            println!("  role:     {}", user.role);
            Ok(())
        }
        Err(deve_sub_application::AuthError::AlreadyInitialized) => {
            // Another process may win after the query; the atomic application
            // operation remains authoritative for first-user creation.
            if args.if_needed {
                println!("Admin initialization skipped: existing users are unchanged.");
                Ok(())
            } else {
                bail!("admin user already exists — use the API or CLI to manage users");
            }
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
    // DS-AUD-028: the three password sources are mutually exclusive. An
    // unused --password in argv still leaks in the process list, so
    // rejecting the combination is a security measure, not just CLI
    // ergonomics.
    let provided = [
        args.password_stdin,
        args.password_env.is_some(),
        args.password.is_some(),
    ]
    .iter()
    .filter(|&&x| x)
    .count();
    if provided > 1 {
        bail!(
            "only one password source may be provided: --password, --password-stdin, \
             and --password-env are mutually exclusive (DS-AUD-028: an unused --password \
             still leaks in the process list)"
        );
    }

    if args.password_stdin {
        let line = stdin_read()?;
        let pw = line.trim_end_matches(['\r', '\n']).to_string();
        if pw.is_empty() {
            bail!("no password read from stdin (stdin was empty)");
        }
        return Ok(pw);
    }

    if let Some(var) = &args.password_env {
        // VarError::NotUnicode embeds the secret value. Never retain that error
        // as a source: the top-level CLI prints the entire anyhow error chain.
        return env_lookup(var)
            .map_err(|_| anyhow!("password env var `{var}` is missing or is not valid UTF-8"));
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
            if_needed: false,
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
            if_needed: false,
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
            if_needed: false,
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
            if_needed: false,
        };

        let resolved = resolve_admin_password_with(&args, NO_ENV, EOF_STDIN).unwrap();
        assert_eq!(resolved, "legacy-pw");
    }

    /// DS-AUD-028: `--password-env` and `--password` are mutually exclusive —
    /// an unused --password still leaks in argv.
    #[test]
    fn password_env_and_argv_both_rejected() {
        let args = UserInitAdminArgs {
            username: "admin".into(),
            password: Some("argv-value".into()),
            password_stdin: false,
            password_env: Some("DEVE_SUB_TEST_ADMIN_PW_PRIORITY".into()),
            db_path: "data/deve-sub.db".into(),
            if_needed: false,
        };
        let lookup = |name: &str| match name {
            "DEVE_SUB_TEST_ADMIN_PW_PRIORITY" => Ok("env-value".to_string()),
            _ => Err(std::env::VarError::NotPresent),
        };

        let err = resolve_admin_password_with(&args, lookup, EOF_STDIN).unwrap_err();
        assert!(
            err.to_string().contains("mutually exclusive"),
            "error should explain mutual exclusion: {err}"
        );
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
            if_needed: false,
        };
        let stdin = || Ok("stdin-pw\n".to_string());

        let resolved = resolve_admin_password_with(&args, NO_ENV, stdin).unwrap();
        assert_eq!(resolved, "stdin-pw");
    }

    /// DS-AUD-028: `--password-stdin` and `--password-env` are mutually
    /// exclusive — only one source is accepted to prevent argv leakage.
    #[test]
    fn password_stdin_and_env_both_rejected() {
        let args = UserInitAdminArgs {
            username: "admin".into(),
            password: None,
            password_stdin: true,
            password_env: Some("DEVE_SUB_TEST_ADMIN_PW_STDIN".into()),
            db_path: "data/deve-sub.db".into(),
            if_needed: false,
        };
        let lookup = |name: &str| match name {
            "DEVE_SUB_TEST_ADMIN_PW_STDIN" => Ok("env-value".to_string()),
            _ => Err(std::env::VarError::NotPresent),
        };
        let stdin = || Ok("stdin-wins\n".to_string());

        let err = resolve_admin_password_with(&args, lookup, stdin).unwrap_err();
        assert!(
            err.to_string().contains("mutually exclusive"),
            "error should explain mutual exclusion: {err}"
        );
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
            if_needed: false,
        };

        let err = resolve_admin_password_with(&args, NO_ENV, EOF_STDIN).unwrap_err();
        assert!(
            err.to_string().contains("stdin was empty"),
            "error should explain empty stdin: {err}"
        );
    }

    /// DS-AUD-028: providing multiple password sources must error — an
    /// unused --password still leaks in argv even if stdin is picked.
    #[test]
    fn password_multiple_sources_rejected() {
        let args = UserInitAdminArgs {
            username: "admin".into(),
            password: Some("argv-leak".into()),
            password_stdin: true,
            password_env: None,
            db_path: "data/deve-sub.db".into(),
            if_needed: false,
        };
        let err = resolve_admin_password_with(&args, NO_ENV, EOF_STDIN).unwrap_err();
        assert!(
            err.to_string().contains("mutually exclusive"),
            "error should explain mutual exclusion: {err}"
        );
    }
}
