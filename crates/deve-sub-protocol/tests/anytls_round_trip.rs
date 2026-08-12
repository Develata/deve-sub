//! Round-trip golden tests for AnyTLS (PARSE-022, PARSE-023).
//!
//! Covers URI parse→emit, Mihomo YAML parse→emit, and sing-box JSON
//! parse→emit. Xray/V2Ray do not support AnyTLS and are excluded with
//! report (constraint #7); the existing `xray_preserves_unknown_protocol`
//! test in `xray_v2ray.rs` uses `anytls` as its fixture, covering that path.

#![allow(clippy::expect_used)]

use deve_sub_domain::{AnyTlsConfig, Authentication, ProtocolConfig, ProtocolKind};

const PASSWORD: &str = "TEST_ANYTLS_PASSWORD";

// --- PARSE-022: AnyTLS URI ---

/// PARSE-022: Parse an `anytls://` URI with full TLS query params.
#[test]
fn anytls_uri_parse_full_fidelity() {
    let uri = format!(
        "anytls://{PASSWORD}@anytls.example.com:443\
         ?sni=anytls.example.com\
         &insecure=1\
         &alpn=h2,http/1.1\
         &fp=chrome\
         #AnyTLS-Test"
    );
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");

    assert_eq!(node.protocol, ProtocolKind::AnyTls);
    assert_eq!(node.display_name, "AnyTLS-Test");
    assert_eq!(node.endpoint.host.uri_host(), "anytls.example.com");
    assert_eq!(node.endpoint.port, 443);

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password auth");
    };
    assert_eq!(password, PASSWORD);

    // AnyTLS always requires TLS; node.tls must be Some with enabled=true.
    let tls = node.tls.as_ref().expect("tls");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("anytls.example.com"));
    assert_eq!(tls.skip_cert_verify, Some(true));
    assert_eq!(tls.alpn, vec!["h2", "http/1.1"]);
    assert_eq!(tls.client_fingerprint.as_deref(), Some("chrome"));

    let ProtocolConfig::AnyTls(AnyTlsConfig {
        idle_session_check_interval,
        idle_session_timeout,
        min_idle_session,
        client_metadata,
    }) = &node.config
    else {
        panic!("expected AnyTls config");
    };
    assert!(idle_session_check_interval.is_none());
    assert!(idle_session_timeout.is_none());
    assert!(min_idle_session.is_none());
    assert!(client_metadata.is_none());
}

/// PARSE-022: Parse → emit → parse yields semantic equality.
#[test]
fn anytls_uri_round_trip_semantic() {
    let uri = format!(
        "anytls://{PASSWORD}@anytls.example.com:443\
         ?sni=anytls.example.com\
         &insecure=0\
         #AnyTLS-RT"
    );
    let parsed1 = deve_sub_protocol::parse_uri(&uri).expect("parse 1");
    let emitted = deve_sub_emitter::emit_uri(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::parse_uri(&emitted).expect("parse 2");

    assert_eq!(parsed1.protocol, parsed2.protocol);
    assert_eq!(parsed1.config, parsed2.config);
    assert_eq!(parsed1.endpoint, parsed2.endpoint);
    assert_eq!(parsed1.authentication, parsed2.authentication);
    assert_eq!(parsed1.display_name, parsed2.display_name);
    assert_eq!(parsed1.tls, parsed2.tls);
}

/// PARSE-022: Missing password returns error.
#[test]
fn anytls_uri_missing_password_errors() {
    let uri = "anytls://@anytls.example.com:443#NoPass";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("password")
    ));
}

/// PARSE-022: Missing port returns error.
#[test]
fn anytls_uri_missing_port_errors() {
    let uri = format!("anytls://{PASSWORD}@anytls.example.com#NoPort");
    let err = deve_sub_protocol::parse_uri(&uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("port")
    ));
}

/// PARSE-022: TLS is always present even with no TLS query params.
#[test]
fn anytls_uri_always_tls() {
    let uri = format!("anytls://{PASSWORD}@anytls.example.com:443#NoTLS");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    let tls = node.tls.as_ref().expect("AnyTLS must always have TLS");
    assert!(tls.enabled);
}

/// PARSE-022: `insecure=0` maps to `skip_cert_verify = Some(false)`.
#[test]
fn anytls_uri_insecure_zero() {
    let uri = format!("anytls://{PASSWORD}@anytls.example.com:443?insecure=0#Insecure0");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    let tls = node.tls.as_ref().expect("tls");
    assert_eq!(tls.skip_cert_verify, Some(false));
}

// --- PARSE-023: Mihomo YAML ---

/// PARSE-023: Parse a Mihomo AnyTLS entry with full fields.
#[test]
fn mihomo_anytls_full_fidelity() {
    let yaml = format!(
        r#"
proxies:
  - name: "AnyTLS-Mihomo"
    type: anytls
    server: anytls.example.com
    port: 443
    password: "{PASSWORD}"
    sni: anytls.example.com
    skip-cert-verify: false
    client-fingerprint: chrome
    alpn: [h2, http/1.1]
    idle-session-check-interval: 30
    idle-session-timeout: 30
    min-idle-session: 0
    udp: true
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::AnyTls);
    assert_eq!(node.display_name, "AnyTLS-Mihomo");
    assert_eq!(node.endpoint.host.uri_host(), "anytls.example.com");
    assert_eq!(node.endpoint.port, 443);

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password");
    };
    assert_eq!(password, PASSWORD);

    let tls = node.tls.as_ref().expect("tls");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("anytls.example.com"));
    assert_eq!(tls.skip_cert_verify, Some(false));
    assert_eq!(tls.client_fingerprint.as_deref(), Some("chrome"));
    assert_eq!(tls.alpn, vec!["h2", "http/1.1"]);

    let ProtocolConfig::AnyTls(cfg) = &node.config else {
        panic!("expected AnyTls config");
    };
    assert_eq!(
        cfg.idle_session_check_interval.map(|d| d.whole_seconds()),
        Some(30)
    );
    assert_eq!(
        cfg.idle_session_timeout.map(|d| d.whole_seconds()),
        Some(30)
    );
    assert_eq!(cfg.min_idle_session, Some(0));
    assert!(cfg.client_metadata.is_none());
}

/// PARSE-023: Mihomo YAML parse → emit → parse yields semantic equality.
#[test]
fn mihomo_anytls_round_trip_semantic() {
    let yaml = format!(
        r#"
proxies:
  - name: "AnyTLS-RT"
    type: anytls
    server: anytls.example.com
    port: 443
    password: "{PASSWORD}"
    sni: anytls.example.com
    skip-cert-verify: true
    idle-session-check-interval: 30
    idle-session-timeout: 30
"#
    );
    let parsed1 = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse 1");
    let emitted = deve_sub_emitter::emit_mihomo(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::container::parse_mihomo_yaml(&emitted).expect("parse 2");

    assert_eq!(parsed1.len(), 1);
    assert_eq!(parsed2.len(), 1);
    assert_eq!(parsed1[0].protocol, parsed2[0].protocol);
    assert_eq!(parsed1[0].config, parsed2[0].config);
    assert_eq!(parsed1[0].endpoint, parsed2[0].endpoint);
    assert_eq!(parsed1[0].authentication, parsed2[0].authentication);
    assert_eq!(parsed1[0].display_name, parsed2[0].display_name);
    assert_eq!(parsed1[0].tls, parsed2[0].tls);
}

// --- PARSE-023: sing-box JSON ---

/// PARSE-023: Parse a sing-box AnyTLS outbound with full fields.
#[test]
fn singbox_anytls_full_fidelity() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "anytls",
      "tag": "AnyTLS-Singbox",
      "server": "anytls.example.com",
      "server_port": 443,
      "password": "{PASSWORD}",
      "tls": {{
        "enabled": true,
        "server_name": "anytls.example.com",
        "insecure": false,
        "alpn": ["h2", "http/1.1"],
        "utls": {{ "enabled": true, "fingerprint": "chrome" }}
      }},
      "idle_session_check_interval": "30s",
      "idle_session_timeout": "30s",
      "min_idle_session": 0,
      "client_metadata": "chrome-120"
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::AnyTls);
    assert_eq!(node.display_name, "AnyTLS-Singbox");
    assert_eq!(node.endpoint.host.uri_host(), "anytls.example.com");
    assert_eq!(node.endpoint.port, 443);

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password");
    };
    assert_eq!(password, PASSWORD);

    let tls = node.tls.as_ref().expect("tls");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("anytls.example.com"));
    assert_eq!(tls.skip_cert_verify, Some(false));
    assert_eq!(tls.alpn, vec!["h2", "http/1.1"]);
    assert_eq!(tls.client_fingerprint.as_deref(), Some("chrome"));

    let ProtocolConfig::AnyTls(cfg) = &node.config else {
        panic!("expected AnyTls config");
    };
    assert_eq!(
        cfg.idle_session_check_interval.map(|d| d.whole_seconds()),
        Some(30)
    );
    assert_eq!(
        cfg.idle_session_timeout.map(|d| d.whole_seconds()),
        Some(30)
    );
    assert_eq!(cfg.min_idle_session, Some(0));
    assert_eq!(cfg.client_metadata.as_deref(), Some("chrome-120"));
}

/// PARSE-023: sing-box JSON parse → emit → parse yields semantic equality.
/// Also exercises the fixed `push_tls_fields` (TLS fields nested under `tls`).
#[test]
fn singbox_anytls_round_trip_semantic() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "anytls",
      "tag": "AnyTLS-RT",
      "server": "anytls.example.com",
      "server_port": 443,
      "password": "{PASSWORD}",
      "tls": {{
        "enabled": true,
        "server_name": "anytls.example.com",
        "insecure": true
      }},
      "idle_session_check_interval": "30s",
      "min_idle_session": 2
    }}
  ]
}}"#
    );
    let parsed1 = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse 1");
    let emitted = deve_sub_emitter::emit_singbox(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::container::parse_singbox_json(&emitted).expect("parse 2");

    assert_eq!(parsed1.len(), 1);
    assert_eq!(parsed2.len(), 1);
    assert_eq!(parsed1[0].protocol, parsed2[0].protocol);
    assert_eq!(parsed1[0].config, parsed2[0].config);
    assert_eq!(parsed1[0].endpoint, parsed2[0].endpoint);
    assert_eq!(parsed1[0].authentication, parsed2[0].authentication);
    assert_eq!(parsed1[0].display_name, parsed2[0].display_name);
    assert_eq!(parsed1[0].tls, parsed2[0].tls);
}

/// PARSE-023: sing-box AnyTLS without explicit TLS block falls back to
/// default-enabled TLS (matches Trojan handling).
#[test]
fn singbox_anytls_default_tls() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "anytls",
      "tag": "AnyTLS-NoTLS",
      "server": "anytls.example.com",
      "server_port": 443,
      "password": "{PASSWORD}"
    }}
  ]
}}"#
    );
    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);
    let tls = nodes[0].tls.as_ref().expect("AnyTLS must always have TLS");
    assert!(tls.enabled);
}

/// Regression: the sing-box `push_tls_fields` fix must emit TLS fields
/// nested under the `tls` object, not as top-level keys. A round-trip of a
/// Trojan node (TLS-bearing) preserves `server_name` and `insecure`.
#[test]
fn singbox_tls_nesting_regression_trojan() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "trojan",
      "tag": "Trojan-TLS",
      "server": "trojan.example.com",
      "server_port": 443,
      "password": "{PASSWORD}",
      "tls": {{
        "enabled": true,
        "server_name": "trojan.example.com",
        "insecure": true
      }}
    }}
  ]
}}"#
    );
    let parsed1 = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse 1");
    let emitted = deve_sub_emitter::emit_singbox(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::container::parse_singbox_json(&emitted).expect("parse 2");

    assert_eq!(parsed2.len(), 1);
    let tls = parsed2[0].tls.as_ref().expect("trojan must retain TLS");
    assert_eq!(tls.server_name.as_deref(), Some("trojan.example.com"));
    assert_eq!(tls.skip_cert_verify, Some(true));
}
