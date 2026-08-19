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

/// Severity of a [`ValidationIssue`] discovered by [`AppConfig::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// The config is unusable; the command must refuse to proceed.
    Error,
    /// The config is likely misconfigured but not fatal; the operator should
    /// review before relying on it.
    Warning,
}

/// A single semantic problem found by [`AppConfig::validate`] (DS-AUD-B08).
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub message: String,
}

impl AppConfig {
    /// Run semantic checks on the resolved config (DS-AUD-B08).
    ///
    /// `config validate` deserializes JSON and prints fields but checks
    /// nothing: bind format, port range, zero TTL/rate-limit, HTTP+Secure
    /// cookie conflict, proxy-header/cookie-secure pairing, and web dist
    /// presence were all silently accepted. This method returns a list of
    /// issues; the caller decides whether Errors are fatal (CLI exit 1) and
    /// whether Warnings are surfaced (log on `serve`).
    ///
    /// This is validate-only and side-effect-free: it does not open the DB,
    /// write files, or bind ports.
    #[must_use]
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        match parse_bind_port(&self.server.bind) {
            Ok(port) => {
                if port == 0 {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        message: format!(
                            "server.bind port 0 is invalid; use 1..=65535 (bind={})",
                            self.server.bind
                        ),
                    });
                }
            }
            Err(e) => issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: format!("server.bind is invalid: {e} (bind={})", self.server.bind),
            }),
        }

        if self.security.session_ttl_secs == 0 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: "security.session_ttl_secs is 0; sessions would expire instantly"
                    .to_owned(),
            });
        }
        if self.security.max_login_attempts == 0 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: "security.max_login_attempts is 0; lockout can never trigger".to_owned(),
            });
        }
        if self.security.lockout_duration_secs == 0 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: "security.lockout_duration_secs is 0; lockout is ineffective".to_owned(),
            });
        }

        let is_loopback = matches!(
            parse_bind_host(&self.server.bind).as_deref(),
            Some("127.0.0.1") | Some("localhost") | Some("::1")
        );
        if self.security.cookie_secure && is_loopback {
            // WHY: a Secure cookie over plain loopback HTTP is never sent
            // back by the browser → /api/v1/auth/me returns 401 after login.
            issues.push(ValidationIssue {
                severity: IssueSeverity::Warning,
                message: "cookie_secure=true with a loopback bind: the browser will not send \
                          the Secure cookie over plain HTTP. Set cookie_secure=false for the \
                          loopback profile, or use a reverse proxy with TLS."
                    .to_owned(),
            });
        }
        if !self.security.cookie_secure && !is_loopback {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Warning,
                message: "cookie_secure=false with a network bind: the session cookie is sent \
                          in the clear over unencrypted HTTP. Use a reverse proxy with TLS and \
                          set cookie_secure=true."
                    .to_owned(),
            });
        }

        if self.security.trust_proxy_headers && !self.security.cookie_secure {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Warning,
                message: "trust_proxy_headers=true but cookie_secure=false: a reverse proxy is \
                          implied (headers trusted) but the cookie is not secured. Likely a \
                          misconfigured reverse-proxy profile."
                    .to_owned(),
            });
        }

        if self.server.serve_web {
            let dist = std::path::Path::new(&self.server.web_dist_dir);
            if !dist.exists() {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Warning,
                    message: format!(
                        "server.serve_web=true but web_dist_dir does not exist: {}. \
                         The placeholder page will be served.",
                        self.server.web_dist_dir
                    ),
                });
            }
        }

        issues
    }
}

fn parse_bind_port(bind: &str) -> Result<u16, String> {
    let port_str = bind.rsplit_once(':').ok_or("expected host:port")?.1;
    port_str
        .parse::<u16>()
        .map_err(|e| format!("port is not a valid u16: {e}"))
}

fn parse_bind_host(bind: &str) -> Option<String> {
    bind.rsplit_once(':').map(|(host, _)| host.to_lowercase())
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

    /// DS-AUD-B08: the default config (loopback HTTP profile) has no
    /// validation Errors. It may have a Warning if web_dist_dir is missing
    /// (common in fresh checkouts), but never an Error.
    #[test]
    fn validate_default_has_no_errors() {
        let config = AppConfig::default();
        let issues = config.validate();
        assert!(
            issues.iter().all(|i| i.severity != IssueSeverity::Error),
            "default config must have no Errors, got: {issues:?}"
        );
    }

    /// DS-AUD-B08: a zero session TTL is an Error (sessions expire instantly).
    #[test]
    fn validate_rejects_zero_session_ttl() {
        let mut config = AppConfig::default();
        config.security.session_ttl_secs = 0;
        let issues = config.validate();
        assert!(issues.iter().any(|i| {
            i.severity == IssueSeverity::Error && i.message.contains("session_ttl_secs")
        }));
    }

    /// DS-AUD-B08: an unparseable bind is an Error.
    #[test]
    fn validate_rejects_invalid_bind() {
        let mut config = AppConfig::default();
        config.server.bind = "not-a-valid-address".to_owned();
        let issues = config.validate();
        assert!(issues.iter().any(|i| {
            i.severity == IssueSeverity::Error && i.message.contains("server.bind is invalid")
        }));
    }

    /// DS-AUD-B08: port 0 is an Error (valid u16 but not a usable port).
    #[test]
    fn validate_rejects_port_zero() {
        let mut config = AppConfig::default();
        config.server.bind = "127.0.0.1:0".to_owned();
        let issues = config.validate();
        assert!(issues.iter().any(|i| {
            i.severity == IssueSeverity::Error && i.message.contains("port 0 is invalid")
        }));
    }

    /// DS-AUD-B08: cookie_secure=true with a loopback bind warns (the
    /// browser won't send the Secure cookie over plain HTTP → login broken).
    #[test]
    fn validate_warns_secure_cookie_on_loopback() {
        let mut config = AppConfig::default();
        config.security.cookie_secure = true;
        let issues = config.validate();
        assert!(issues.iter().any(|i| {
            i.severity == IssueSeverity::Warning && i.message.contains("loopback bind")
        }));
    }

    /// DS-AUD-B08: cookie_secure=false with a network bind warns (cookie
    /// sent in the clear over unencrypted HTTP).
    #[test]
    fn validate_warns_insecure_cookie_on_network() {
        let mut config = AppConfig::default();
        config.server.bind = "0.0.0.0:8080".to_owned();
        let issues = config.validate();
        assert!(issues.iter().any(|i| {
            i.severity == IssueSeverity::Warning && i.message.contains("in the clear")
        }));
    }

    /// DS-AUD-B08: trust_proxy_headers=true without cookie_secure warns
    /// (reverse-proxy profile is half-configured).
    #[test]
    fn validate_warns_proxy_headers_without_secure_cookie() {
        let mut config = AppConfig::default();
        config.server.bind = "0.0.0.0:8080".to_owned();
        config.security.trust_proxy_headers = true;
        let issues = config.validate();
        // WHY: the proxy-headers warning fires alongside the network-insecure
        // warning; assert at least the proxy-headers one is present.
        assert!(issues.iter().any(|i| {
            i.severity == IssueSeverity::Warning && i.message.contains("trust_proxy_headers")
        }));
    }
}
