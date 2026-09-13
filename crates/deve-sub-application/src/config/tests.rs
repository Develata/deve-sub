use super::*;

#[test]
fn audit_005_retention_defaults_disable_and_validation() {
    let default = AppConfig::default();
    let old: AppConfig = serde_json::from_str("{}").expect("old config");
    assert_eq!(default.logging.audit_retention_days, 90);
    assert_eq!(old.logging.audit_retention_days, 90);
    for days in [0, 1, 90, 3650] {
        let config: AppConfig =
            serde_json::from_value(serde_json::json!({"logging": {"audit_retention_days": days}}))
                .expect("config");
        assert!(
            !config
                .validate()
                .iter()
                .any(|i| i.severity == IssueSeverity::Error)
        );
    }
    let mut config = default;
    config.logging.audit_retention_days = 3651;
    assert!(
        config
            .validate()
            .iter()
            .any(|i| i.severity == IssueSeverity::Error
                && i.message.contains("audit_retention_days"))
    );
    assert!(
        serde_json::from_str::<AppConfig>(r#"{"logging":{"audit_retention_days":-1}}"#).is_err()
    );
}

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
    assert!(
        issues.iter().any(|i| {
            i.severity == IssueSeverity::Error && i.message.contains("session_ttl_secs")
        })
    );
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
    assert!(
        issues.iter().any(|i| {
            i.severity == IssueSeverity::Warning && i.message.contains("loopback bind")
        })
    );
}

/// DS-AUD-B08: cookie_secure=false with a network bind warns (cookie
/// sent in the clear over unencrypted HTTP).
#[test]
fn validate_warns_insecure_cookie_on_network() {
    let mut config = AppConfig::default();
    config.server.bind = "0.0.0.0:8080".to_owned();
    let issues = config.validate();
    assert!(
        issues.iter().any(|i| {
            i.severity == IssueSeverity::Warning && i.message.contains("in the clear")
        })
    );
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

/// P0-06: `deny_unknown_fields` rejects top-level typos so a misspelled
/// config key is caught at load instead of being silently ignored.
#[test]
fn deny_unknown_fields_rejects_top_level_typo() {
    let json = r#"{"prodcut_name": "Deve Sub"}"#;
    let result: Result<AppConfig, _> = serde_json::from_str(json);
    assert!(result.is_err(), "unknown top-level field must be rejected");
    let msg = result.expect_err("already asserted is_err").to_string();
    assert!(
        msg.contains("unknown field") && msg.contains("prodcut_name"),
        "error should name the unknown field, got: {msg}"
    );
}

/// P0-06: `deny_unknown_fields` rejects typos in nested config sections.
#[test]
fn deny_unknown_fields_rejects_nested_typo() {
    let json = r#"{"server": {"bind": "127.0.0.1:8080", "bimd": "0.0.0.0:9090"}}"#;
    let result: Result<AppConfig, _> = serde_json::from_str(json);
    assert!(result.is_err(), "unknown nested field must be rejected");
    let msg = result.expect_err("already asserted is_err").to_string();
    assert!(
        msg.contains("unknown field") && msg.contains("bimd"),
        "error should name the unknown nested field, got: {msg}"
    );
}

/// P0-06: a minimal valid config with only known fields deserializes
/// successfully (regression guard alongside the rejection tests).
#[test]
fn deny_unknown_fields_accepts_minimal_valid_config() {
    let json = r#"{}"#;
    let config: AppConfig = serde_json::from_str(json).expect("empty JSON must use serde defaults");
    assert_eq!(config.server.bind, "127.0.0.1:8080");
}
