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

    /// GeoIP configuration for auto-region detection.
    #[serde(default)]
    pub geoip: GeoIpConfig,
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

    /// Whether to auto-generate the master key if the file is missing.
    ///
    /// When `true` (the default), a missing key file is silently generated
    /// with a warning — convenient for local development. When `false`, the
    /// server refuses to start if the key file does not exist, preventing
    /// silent key loss in production (which would invalidate all sessions,
    /// recovery codes, and encrypted TOTP secrets).
    ///
    /// WHY: `load_or_generate` on a missing key file creates a new key with
    /// only a `tracing::warn`. If the path is misconfigured or the mount is
    /// lost, the server starts with a fresh key and all existing
    /// HMAC-derived data (sessions, recovery codes) and encrypted data
    /// (TOTP secrets) become unrecoverable. Setting this to `false` in
    /// production makes such misconfiguration a hard failure.
    #[serde(default = "default_allow_master_key_generation")]
    pub allow_master_key_generation: bool,

    /// Session lifetime in seconds. Default: 86400 (24 hours).
    #[serde(default = "default_session_ttl_secs")]
    pub session_ttl_secs: u64,

    /// Whether to set the `Secure` flag on session cookies.
    ///
    /// When `true` (the default), cookies are only sent over HTTPS. Disable
    /// only for local development over plain HTTP by setting
    /// `cookie_secure: false` in the config file or `DEVE_SUB_COOKIE_SECURE=0`.
    /// Production deployments must keep this enabled.
    ///
    /// WHY: with `cookie_secure: true` and a plain-HTTP bind (the default
    /// `0.0.0.0:8080`), browsers and cookie-aware HTTP clients (e.g. curl
    /// with a cookie jar) will not send the `Secure` cookie back, causing
    /// `/api/v1/auth/me` to return 401. For local dev without TLS, set
    /// `cookie_secure: false`.
    #[serde(default = "default_cookie_secure")]
    pub cookie_secure: bool,

    /// Maximum failed login attempts before temporary lockout (AUTH-004).
    #[serde(default = "default_max_login_attempts")]
    pub max_login_attempts: u32,

    /// Lockout duration in seconds after exceeding `max_login_attempts`
    /// (AUTH-004).
    #[serde(default = "default_lockout_duration_secs")]
    pub lockout_duration_secs: u64,

    /// Whether to trust `X-Forwarded-For` / `X-Real-IP` headers for client
    /// IP extraction (SEC-007).
    ///
    /// When `false` (the default), proxy headers are **ignored** — the
    /// server does not use them for IP-based rate limiting. This prevents
    /// IP spoofing by clients sending fake headers directly to the server.
    ///
    /// When `true`, the server trusts the last hop of `X-Forwarded-For` and
    /// the `X-Real-IP` header. Enable this only when the server is behind a
    /// trusted reverse proxy (Caddy, nginx) that overwrites client-sent
    /// values.
    #[serde(default = "default_trust_proxy_headers")]
    pub trust_proxy_headers: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            product_name: default_product_name(),
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            security: SecurityConfig::default(),
            geoip: GeoIpConfig::default(),
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
            allow_master_key_generation: default_allow_master_key_generation(),
            session_ttl_secs: default_session_ttl_secs(),
            cookie_secure: default_cookie_secure(),
            max_login_attempts: default_max_login_attempts(),
            lockout_duration_secs: default_lockout_duration_secs(),
            trust_proxy_headers: default_trust_proxy_headers(),
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

fn default_allow_master_key_generation() -> bool {
    true
}

fn default_session_ttl_secs() -> u64 {
    86_400
}

fn default_cookie_secure() -> bool {
    true
}

fn default_max_login_attempts() -> u32 {
    5
}

fn default_lockout_duration_secs() -> u64 {
    300
}

fn default_trust_proxy_headers() -> bool {
    false
}

/// GeoIP configuration for auto-region detection (NODE-007/008/009).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeoIpConfig {
    /// Path to the MaxMind `.mmdb` database file. `None` or missing file
    /// disables auto-region (graceful degradation).
    pub mmdb_path: Option<String>,
}
