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
    ///
    /// DS-AUD-B02: default is `127.0.0.1:8080` (loopback) so the
    /// "one-command" path (`deve-sub serve` with no config) is safe and
    /// usable: a browser on `http://localhost:8080` can init, login,
    /// refresh, and logout. To expose the server to the network, set
    /// `bind: "0.0.0.0:8080"` in the config file or pass `--bind` — and
    /// put it behind a reverse proxy with TLS for production (see the
    /// `cookie_secure` field for the matching cookie contract).
    #[serde(default = "default_bind_addr")]
    pub bind: String,

    /// Serve web UI (false for headless mode).
    #[serde(default = "default_serve_web")]
    pub serve_web: bool,

    /// Path to the compiled web frontend dist directory. When `serve_web`
    /// is true, static files are served from this directory. Falls back
    /// to the placeholder HTML if the directory does not exist.
    #[serde(default = "default_web_dist_dir")]
    pub web_dist_dir: String,
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
    /// DS-AUD-B02: default is `false` so the "one-command" path
    /// (`deve-sub serve` with no config, defaulting to loopback HTTP) works:
    /// browsers will send the cookie back over plain HTTP. With `true` and a
    /// plain-HTTP bind, the browser never sends the session cookie, so
    /// `/api/v1/auth/me` returns 401 after a successful login — the
    /// "login is broken" trap.
    ///
    /// For production behind a reverse proxy (Caddy/nginx terminating TLS),
    /// set `cookie_secure: true` in the config file. The reverse proxy
    /// provides the HTTPS hop the browser needs to send the cookie back.
    /// See `trust_proxy_headers` for the matching IP-extraction setting.
    ///
    /// WHY: the previous default (`true`) made the default deployment
    /// (HTTP) unusable for browser login. Operators had to read a code
    /// comment to discover the mismatch. The new default pairs the cookie
    /// flag with the bind default so the "one-command" path is correct by
    /// construction; production opts into `Secure` via the config file.
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
            web_dist_dir: default_web_dist_dir(),
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
    // WHY (DS-AUD-B02): loopback default so `deve-sub serve` with no config
    // is safe and usable for browser login over plain HTTP. Network exposure
    // requires an explicit `--bind 0.0.0.0:...` or config-file opt-in.
    "127.0.0.1:8080".to_owned()
}

fn default_serve_web() -> bool {
    true
}

fn default_web_dist_dir() -> String {
    "apps/web/dist".to_owned()
}

fn default_db_path() -> String {
    "data/deve-sub.db".to_owned()
}

fn default_master_key_path() -> String {
    "data/master.key".to_owned()
}

fn default_allow_master_key_generation() -> bool {
    // WHY default false: prevents silent key rotation on a lost key. If the
    // key file is missing and generation is allowed, a new key is created
    // and all existing encrypted columns become unreadable. Defaulting to
    // fail-closed forces the operator to explicitly opt in (ADR-0007 §7).
    false
}

fn default_session_ttl_secs() -> u64 {
    86_400
}

fn default_cookie_secure() -> bool {
    // WHY (DS-AUD-B02): false pairs with the loopback-HTTP default bind so
    // the "one-command" path works for browser login. A Secure cookie over
    // plain HTTP is never sent back by the browser, so /api/v1/auth/me
    // returns 401 after a successful login — the "login is broken" trap.
    // Production behind a reverse proxy opts into true via the config file.
    false
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

#[cfg(test)]
mod tests {
    use super::*;

    /// DS-AUD-B02: the "one-command" path (`deve-sub serve` with no config)
    /// must be safe and usable for browser login over plain HTTP. The
    /// defaults pair: loopback bind + non-Secure cookie. A browser on
    /// `http://localhost:8080` can init, login, refresh, and logout.
    #[test]
    fn default_config_is_loopback_http_with_insecure_cookie() {
        let config = AppConfig::default();
        assert_eq!(
            config.server.bind, "127.0.0.1:8080",
            "default bind must be loopback so the one-command HTTP path is safe"
        );
        assert!(
            !config.security.cookie_secure,
            "default cookie_secure must be false so the browser sends the cookie over HTTP"
        );
    }

    /// DS-AUD-B02: round-trip through serde preserves the defaults (no
    /// regression if the deserializer defaults drift from `Default::default()`).
    #[test]
    fn default_config_round_trips_through_serde() {
        let json = serde_json::to_string(&AppConfig::default()).expect("serialize");
        let back: AppConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.server.bind, "127.0.0.1:8080");
        assert!(!back.security.cookie_secure);
    }

    /// DS-AUD-B02: an empty JSON object deserializes to the loopback-HTTP
    /// profile (the serde defaults must match the `Default` impl).
    #[test]
    fn empty_json_uses_loopback_http_defaults() {
        let config: AppConfig = serde_json::from_str("{}").expect("empty json");
        assert_eq!(config.server.bind, "127.0.0.1:8080");
        assert!(!config.security.cookie_secure);
    }
}
