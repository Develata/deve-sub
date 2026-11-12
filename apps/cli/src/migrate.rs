//! Explicit schema and keyed generation-cache migration.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

#[path = "migrate_preflight.rs"]
pub(crate) mod preflight;
#[path = "migrate_security.rs"]
mod security;
#[path = "migrate_upgrade.rs"]
mod upgrade;
pub(crate) use upgrade::open_current_database;

/// Migrate command arguments.
#[derive(Args)]
pub struct MigrateArgs {
    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH", default_value = "data/deve-sub.db")]
    db_path: String,

    /// Existing master key for encrypting legacy generation-cache payloads.
    /// Defaults to data/master.key when the database contains cached output.
    #[arg(long, env = "DEVE_SUB_KEY_PATH")]
    key_path: Option<String>,
    /// Parent for a new private rollback directory (default: database parent).
    #[arg(long)]
    rollback_dir: Option<PathBuf>,
    /// Reviewed exact old-NodeId quarantine/TLS recovery mapping.
    #[arg(long)]
    security_map: Option<PathBuf>,
    /// Verify schema and the exact mapping on private staging, then discard it.
    #[arg(long, requires = "security_map")]
    preview: bool,
}

pub async fn migrate(args: MigrateArgs) -> Result<()> {
    upgrade::run(args).await
}
