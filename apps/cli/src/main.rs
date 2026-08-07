//! Deve Sub CLI entry point.
//!
//! Subcommands: `serve`, `serve --headless`, `doctor`, `migrate`,
//! `config validate`, `openapi`, `user init-admin`, `source add`. See
//! `docs/plan/milestones/M1-infrastructure.md`,
//! `docs/plan/milestones/M2-auth-and-users.md`, and
//! `docs/plan/milestones/M4-sources-and-node-pool.md`.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod commands;
mod node_cmds;

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
    /// Export OpenAPI specification to stdout.
    Openapi(commands::OpenapiArgs),
    /// User management commands.
    User(commands::UserArgs),
    /// Source management commands.
    Source(commands::SourceArgs),
    /// Node pool commands.
    Node(commands::NodeArgs),
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
            Commands::Serve(args) => commands::serve(args).await,
            Commands::Doctor(args) => commands::doctor(args).await,
            Commands::Migrate(args) => commands::migrate(args).await,
            Commands::Config(args) => match args.command {
                commands::ConfigSubCommand::Validate(sub) => commands::config_validate(sub).await,
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
