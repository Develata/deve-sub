//! Serve configuration overrides, including bounded audit retention.

use clap::Args;
use deve_sub_application::AppConfig;
use std::path::PathBuf;

/// Start the HTTP server.
#[derive(Args)]
pub struct ServeArgs {
    /// Path to configuration file.
    #[arg(long, env = "DEVE_SUB_CONFIG")]
    pub(crate) config: Option<PathBuf>,

    /// Bind address.
    #[arg(long, env = "DEVE_SUB_BIND")]
    pub(crate) bind: Option<String>,

    /// Days of audit history to retain automatically (0 disables; default 90).
    #[arg(long, env = "DEVE_SUB_AUDIT_RETENTION_DAYS", value_parser = clap::value_parser!(u32).range(0..=3650))]
    pub(crate) audit_retention_days: Option<u32>,

    /// Run without web UI (API and subscription only).
    #[arg(long)]
    pub(crate) headless: bool,

    /// Database path.
    #[arg(long, env = "DEVE_SUB_DB_PATH")]
    pub(crate) db_path: Option<String>,

    /// Path to the master key file (overrides `security.master_key_path`).
    ///
    /// DS-AUD-B01: explicit key path removes the relative-path resolution
    /// footgun where systemd `WorkingDirectory=$DATA_DIR` made the default
    /// `data/master.key` resolve to `$DATA_DIR/data/master.key`.
    #[arg(long, env = "DEVE_SUB_KEY_PATH")]
    pub(crate) key_path: Option<String>,

    /// Path to the compiled web frontend dist directory.
    #[arg(long, env = "DEVE_SUB_WEB_DIST_DIR")]
    pub(crate) web_dist_dir: Option<String>,
}

impl ServeArgs {
    /// Apply CLI overrides to the loaded configuration in one place.
    pub(crate) fn apply_overrides(&self, config: &mut AppConfig) {
        if let Some(days) = self.audit_retention_days {
            config.logging.audit_retention_days = days;
        }
        if let Some(bind) = &self.bind {
            config.server.bind = bind.clone();
        }
        if self.headless {
            config.server.serve_web = false;
        }
        if let Some(db_path) = &self.db_path {
            config.database.path = db_path.clone();
        }
        if let Some(key_path) = &self.key_path {
            config.security.master_key_path = key_path.clone();
        }
        if let Some(web_dist_dir) = &self.web_dist_dir {
            config.server.web_dist_dir = web_dist_dir.clone();
        }
    }
}
