//! TLS configuration with three-state certificate verification.
//!
//! `skip_cert_verify` uses `Option<bool>` to distinguish "not provided"
//! (`None`) from "explicitly secure" (`Some(false)`) and "explicitly insecure"
//! (`Some(true)`). The system must never auto-set `Some(true)` for
//! compatibility. See ADR-0005.

use serde::{Deserialize, Serialize};

/// TLS settings for a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsConfig {
    pub enabled: bool,
    pub server_name: Option<String>,
    /// Three-state certificate verification:
    /// - `None` — not provided; do not fill a default.
    /// - `Some(false)` — explicitly secure (e.g. `allowInsecure=0`).
    /// - `Some(true)` — explicitly insecure (e.g. `allowInsecure=1`).
    pub skip_cert_verify: Option<bool>,
    pub alpn: Vec<String>,
    pub client_fingerprint: Option<String>,
    pub certificate_pins: Vec<CertificatePin>,
    pub reality: Option<RealityConfig>,
}

/// Reality TLS extension configuration (used by VLESS Reality).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealityConfig {
    /// Public key (`pbk`), Base64URL-encoded. Character-set validation is
    /// deferred to the M3 parsing layer (plan/05 §6.1).
    pub public_key: String,
    /// Short ID (`sid`). Always stored as a string; YAML must not coerce
    /// pure-digit short IDs to integers.
    pub short_id: String,
    /// Spider X path (`spx`).
    pub spider_x: Option<String>,
}

/// A certificate pin (e.g. `pinSHA256`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CertificatePin(String);

impl CertificatePin {
    /// Create a pin from a string (e.g. `pinSHA256:...`).
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// Return the underlying pin string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_state_skip_cert_verify() {
        let not_provided = TlsConfig {
            enabled: true,
            server_name: None,
            skip_cert_verify: None,
            alpn: vec![],
            client_fingerprint: None,
            certificate_pins: vec![],
            reality: None,
        };
        assert!(not_provided.skip_cert_verify.is_none());

        let secure = TlsConfig {
            skip_cert_verify: Some(false),
            ..not_provided.clone()
        };
        assert_eq!(secure.skip_cert_verify, Some(false));

        let insecure = TlsConfig {
            skip_cert_verify: Some(true),
            ..not_provided
        };
        assert_eq!(insecure.skip_cert_verify, Some(true));
    }

    #[test]
    fn reality_short_id_is_string() {
        let reality = RealityConfig {
            public_key: "TEST_PUBLIC_KEY".to_owned(),
            short_id: "01020304".to_owned(),
            spider_x: None,
        };
        assert_eq!(reality.short_id, "01020304");
    }

    #[test]
    fn tls_config_serde_roundtrip() {
        let tls = TlsConfig {
            enabled: true,
            server_name: Some("example.com".to_owned()),
            skip_cert_verify: Some(false),
            alpn: vec!["h2".to_owned(), "http/1.1".to_owned()],
            client_fingerprint: Some("chrome".to_owned()),
            certificate_pins: vec![CertificatePin::new("pinSHA256:abc123".to_owned())],
            reality: Some(RealityConfig {
                public_key: "TEST_PUBLIC_KEY".to_owned(),
                short_id: "01020304".to_owned(),
                spider_x: Some("/".to_owned()),
            }),
        };
        let json = serde_json::to_string(&tls).expect("serialize");
        let recovered: TlsConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(tls, recovered);
    }
}
