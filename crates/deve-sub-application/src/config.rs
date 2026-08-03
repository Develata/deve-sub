//! Application configuration loading from file and environment.
//!
//! The product name is centralized here; no hardcoded scattering. See
//! `docs/plan/00-engineering-constitution.md` §"Naming".

use serde::{Deserialize, Serialize};

/// Root application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Product display name, centralized here to avoid hardcoded scattering.
    #[serde(default = "default_product_name")]
    pub product_name: String,

    /// Server bind configuration.
    #[serde(default)]
    pub server: ServerConfig,

    /// Database configuration.
    #[serde(default)]
    pub database: DatabaseConfig,
}

/// HTTP server bind configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Bind address.
    #[serde(default = "default_bind_addr")]
    pub bind: String,

    /// Serve web UI (false for headless mode).
    #[serde(default = "default_serve_web")]
    pub serve_web: bool,
}

/// Database configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// SQLite database file path.
    #[serde(default = "default_db_path")]
    pub path: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            product_name: default_product_name(),
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind_addr(),
            serve_web: default_serve_web(),
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: default_db_path(),
        }
    }
}

fn default_product_name() -> String {
    "Deve Sub".to_owned()
}

fn default_bind_addr() -> String {
    "0.0.0.0:8080".to_owned()
}

fn default_serve_web() -> bool {
    true
}

fn default_db_path() -> String {
    "data/deve-sub.db".to_owned()
}
