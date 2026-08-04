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

    /// Security configuration.
    #[serde(default)]
    pub security: SecurityConfig,
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

/// Security configuration for auth, HMAC, and encryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Path to the master key file (32 bytes of random data).
    /// Used for HMAC-SHA256 of session tokens and XChaCha20-Poly1305
    /// encryption of sensitive fields. See
    /// `docs/plan/00-engineering-constitution.md` §"Data and security".
    #[serde(default = "default_master_key_path")]
    pub master_key_path: String,

    /// Session lifetime in seconds. Default: 86400 (24 hours).
    #[serde(default = "default_session_ttl_secs")]
    pub session_ttl_secs: u64,

    /// Whether to set the `Secure` flag on session cookies.
    ///
    /// When `true` (the default), cookies are only sent over HTTPS. Disable
    /// only for local development over plain HTTP. Production deployments
    /// must keep this enabled.
    #[serde(default = "default_cookie_secure")]
    pub cookie_secure: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            product_name: default_product_name(),
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            security: SecurityConfig::default(),
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

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            master_key_path: default_master_key_path(),
            session_ttl_secs: default_session_ttl_secs(),
            cookie_secure: default_cookie_secure(),
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

fn default_master_key_path() -> String {
    "data/master.key".to_owned()
}

fn default_session_ttl_secs() -> u64 {
    86_400
}

fn default_cookie_secure() -> bool {
    true
}
