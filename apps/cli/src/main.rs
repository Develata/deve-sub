//! Deve Sub CLI entry point.
//!
//! Subcommands: `serve`, `serve --headless`, `doctor`, `migrate`,
//! `config validate`, `openapi`, `user init-admin`, `source add`. See
//! `docs/plan/milestones/M1-infrastructure.md`,
//! `docs/plan/milestones/M2-auth-and-users.md`, and
//! `docs/plan/milestones/M4-sources-and-node-pool.md`.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod backup;
mod commands;
mod db_lock;
mod health;
mod node_cmds;
mod serve;
mod subscription_cmds;
mod template_cmds;
mod update;

/// Deve Sub — self-hosted proxy subscription infrastructure manager.
#[derive(Parser)]
#[command(name = "deve-sub", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the HTTP server.
    Serve(commands::ServeArgs),
    /// Run system diagnostics.
    Doctor(commands::DoctorArgs),
    /// Apply database migrations.
    Migrate(commands::MigrateArgs),
    /// Configuration commands.
    Config(commands::ConfigArgs),
    /// Master key management commands.
    Key(commands::KeyArgs),
    /// Export OpenAPI specification to stdout.
    Openapi(commands::OpenapiArgs),
    /// User management commands.
    User(commands::UserArgs),
    /// Source management commands.
    Source(commands::SourceArgs),
    /// Node pool commands.
    Node(commands::NodeArgs),
    /// Template management commands.
    Template(commands::TemplateArgs),
    /// Subscription management commands.
    Subscription(subscription_cmds::SubscriptionArgs),
    /// Create a full-state backup archive.
    Backup(backup::BackupArgs),
    /// Restore database from a backup archive.
    Restore(backup::RestoreArgs),
    /// HTTP health probes for Docker HEALTHCHECK.
    Health(health::HealthArgs),
    /// Self-update the binary from the latest GitHub release.
    Update(update::UpdateArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("failed to start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = runtime.block_on(async {
        deve_sub_observability::init_tracing()?;
        match cli.command {
            Commands::Serve(args) => serve::serve(args).await,
            Commands::Doctor(args) => commands::doctor(args).await,
            Commands::Migrate(args) => commands::migrate(args).await,
            Commands::Config(args) => match args.command {
                commands::ConfigSubCommand::Validate(sub) => commands::config_validate(sub).await,
            },
            Commands::Key(args) => match args.command {
                commands::KeySubCommand::Init(sub) => commands::key_init(sub).await,
            },
            Commands::Openapi(args) => commands::openapi(args).await,
            Commands::User(args) => match args.command {
                commands::UserSubCommand::InitAdmin(sub) => commands::user_init_admin(sub).await,
            },
            Commands::Source(args) => match args.command {
                commands::SourceSubCommand::Add(sub) => commands::source_add(sub).await,
            },
            Commands::Node(args) => match args.command {
                commands::NodeSubCommand::Import(sub) => commands::node_import(sub).await,
                commands::NodeSubCommand::List(sub) => commands::node_list(sub).await,
            },
            Commands::Template(args) => match args.command {
                commands::TemplateSubCommand::Add(sub) => commands::template_add(sub).await,
                commands::TemplateSubCommand::List(sub) => commands::template_list(sub).await,
                commands::TemplateSubCommand::Get(sub) => commands::template_get(sub).await,
                commands::TemplateSubCommand::Update(sub) => commands::template_update(sub).await,
                commands::TemplateSubCommand::Delete(sub) => commands::template_delete(sub).await,
                commands::TemplateSubCommand::Versions(sub) => {
                    commands::template_versions(sub).await
                }
                commands::TemplateSubCommand::Rollback(sub) => {
                    commands::template_rollback(sub).await
                }
            },
            Commands::Subscription(args) => match args.command {
                subscription_cmds::SubscriptionSubCommand::Add(sub) => {
                    subscription_cmds::subscription_add(sub).await
                }
                subscription_cmds::SubscriptionSubCommand::List(sub) => {
                    subscription_cmds::subscription_list(sub).await
                }
                subscription_cmds::SubscriptionSubCommand::Get(sub) => {
                    subscription_cmds::subscription_get(sub).await
                }
                subscription_cmds::SubscriptionSubCommand::Update(sub) => {
                    subscription_cmds::subscription_update(sub).await
                }
                subscription_cmds::SubscriptionSubCommand::Delete(sub) => {
                    subscription_cmds::subscription_delete(sub).await
                }
                subscription_cmds::SubscriptionSubCommand::RotateToken(sub) => {
                    subscription_cmds::subscription_rotate(sub).await
                }
            },
            Commands::Health(args) => match args.command {
                health::HealthSubCommand::Live(a) => health::health_live(a).await,
                health::HealthSubCommand::Ready(a) => health::health_ready(a).await,
            },
            Commands::Backup(args) => backup::backup(args).await,
            Commands::Restore(args) => backup::restore(args).await,
            Commands::Update(args) => update::update(args).await,
        }
    });

    if let Err(e) = result {
        tracing::error!("command failed: {e:#}");
        eprintln!("error: {e:#}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
