//! Golden tests for Mihomo YAML parser (PARSE-005).

#![allow(clippy::expect_used)]

use deve_sub_domain::{Authentication, ProtocolConfig, ProtocolKind, TransportKind, UdpRelayMode};

const RESERVED_UUID: &str = "00000000-0000-4000-8000-000000000001";
const RESERVED_PASSWORD: &str = "TEST_PASSWORD";

/// PARSE-005: Parse a VLESS Reality entry with IPv6.
#[test]
fn mihomo_parses_vless_reality_ipv6() {
    let yaml = format!(
        r#"
proxies:
  - name: "VLESS-IPv6"
    type: vless
    server: "2001:db8::1"
    port: 443
    uuid: "{RESERVED_UUID}"
    network: tcp
    tls: true
    servername: example.com
    flow: xtls-rprx-vision
    client-fingerprint: chrome
    reality-opts:
      public-key: TEST_PUBLIC_KEY
      short-id: "01020304"
    skip-cert-verify: false
    udp: true
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Vless);
    assert_eq!(node.display_name, "VLESS-IPv6");
    assert_eq!(node.endpoint.host.uri_host(), "[2001:db8::1]");
    assert_eq!(node.endpoint.port, 443);

    let Authentication::Uuid { uuid } = &node.authentication else {
        panic!("expected Uuid auth");
    };
    assert_eq!(uuid, RESERVED_UUID);

    let ProtocolConfig::VlessReality(cfg) = &node.config else {
        panic!("expected VlessReality config");
    };
    assert_eq!(cfg.flow.as_deref(), Some("xtls-rprx-vision"));

    let tls = node.tls.as_ref().expect("tls");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("example.com"));
    assert_eq!(tls.client_fingerprint.as_deref(), Some("chrome"));
    assert_eq!(tls.skip_cert_verify, Some(false));

    let reality = tls.reality.as_ref().expect("reality");
    assert_eq!(reality.public_key, "TEST_PUBLIC_KEY");
    assert_eq!(reality.short_id, "01020304");

    assert_eq!(node.udp.supported, Some(true));
}

/// PARSE-005: Parse a Trojan entry with WebSocket transport.
#[test]
fn mihomo_parses_trojan_ws() {
    let yaml = format!(
        r#"
proxies:
  - name: "Trojan-WS"
    type: trojan
    server: example.com
    port: 443
    password: "{RESERVED_PASSWORD}"
    network: ws
    sni: example.com
    alpn: [h2, http/1.1]
    skip-cert-verify: false
    ws-opts:
      path: /ws
      headers:
        Host: ws.example.com
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Trojan);

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password auth");
    };
    assert_eq!(password, RESERVED_PASSWORD);

    let transport = node.transport.as_ref().expect("transport");
    assert_eq!(transport.kind, TransportKind::Ws);
    assert_eq!(transport.path.as_deref(), Some("/ws"));
    assert_eq!(transport.host.as_deref(), Some("ws.example.com"));

    let tls = node.tls.as_ref().expect("tls");
    assert_eq!(tls.alpn, vec!["h2", "http/1.1"]);
}

/// PARSE-005: Parse a Shadowsocks entry.
#[test]
fn mihomo_parses_shadowsocks() {
    let yaml = format!(
        r#"
proxies:
  - name: "SS-Node"
    type: ss
    server: ss.example.com
    port: 8388
    cipher: aes-256-gcm
    password: "{RESERVED_PASSWORD}"
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Shadowsocks);

    let ProtocolConfig::Shadowsocks(cfg) = &node.config else {
        panic!("expected Shadowsocks config");
    };
    assert_eq!(cfg.method, "aes-256-gcm");
}

/// PARSE-005: Parse a VMess entry.
#[test]
fn mihomo_parses_vmess() {
    let yaml = format!(
        r#"
proxies:
  - name: "VMess-Node"
    type: vmess
    server: vmess.example.com
    port: 443
    uuid: "{RESERVED_UUID}"
    alterId: 0
    cipher: auto
    network: ws
    tls: true
    servername: vmess.example.com
    ws-opts:
      path: /vmess
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::VMess);

    let ProtocolConfig::VMess(cfg) = &node.config else {
        panic!("expected VMess config");
    };
    assert_eq!(cfg.alter_id, Some(0));
    assert_eq!(cfg.security.as_deref(), Some("auto"));

    let transport = node.transport.as_ref().expect("transport");
    assert_eq!(transport.kind, TransportKind::Ws);
    assert_eq!(transport.path.as_deref(), Some("/vmess"));
}

/// PARSE-005: Parse a Hysteria2 entry.
#[test]
fn mihomo_parses_hysteria2() {
    let yaml = format!(
        r#"
proxies:
  - name: "Hy2-Node"
    type: hysteria2
    server: hy2.example.com
    port: 443
    password: "{RESERVED_PASSWORD}"
    sni: hy2.example.com
    up: "100 Mbps"
    down: "200 Mbps"
    obfs: salamander
    obfs-password: obfs-pwd
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Hysteria2);

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password auth");
    };
    assert_eq!(password, RESERVED_PASSWORD);

    let obfs = node.obfuscation.as_ref().expect("obfs");
    assert_eq!(obfs.kind, "salamander");
    assert_eq!(obfs.password.as_deref(), Some("obfs-pwd"));

    let congestion = node.congestion.as_ref().expect("congestion");
    assert_eq!(congestion.up_bps, Some(100_000_000));
    assert_eq!(congestion.down_bps, Some(200_000_000));
}

/// PARSE-005: Parse a TUIC v5 entry.
#[test]
fn mihomo_parses_tuic_v5() {
    let yaml = format!(
        r#"
proxies:
  - name: "TUIC-Node"
    type: tuic
    server: tuic.example.com
    port: 443
    uuid: "{RESERVED_UUID}"
    password: "{RESERVED_PASSWORD}"
    congestion-controller: bbr
    udp-relay-mode: native
    alpn: [h3]
    sni: tuic.example.com
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::TuicV5);

    let Authentication::UuidPassword { uuid, password } = &node.authentication else {
        panic!("expected UuidPassword auth");
    };
    assert_eq!(uuid, RESERVED_UUID);
    assert_eq!(password, RESERVED_PASSWORD);

    let ProtocolConfig::TuicV5(cfg) = &node.config else {
        panic!("expected TuicV5 config");
    };
    assert_eq!(cfg.udp_relay_mode, Some(UdpRelayMode::Native));

    let congestion = node.congestion.as_ref().expect("congestion");
    assert!(matches!(
        congestion.controller,
        deve_sub_domain::CongestionController::Bbr
    ));
}

/// PARSE-005: Unknown proxy type is preserved as UnsupportedNode.
#[test]
fn mihomo_preserves_unknown_type() {
    let yaml = r#"
proxies:
  - name: "Unknown"
    type: snell
    server: example.com
    port: 443
"#;

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(yaml).expect("parse");
    assert_eq!(nodes.len(), 1);
    assert!(matches!(nodes[0].config, ProtocolConfig::Unsupported(_)));
}

/// Missing `proxies` key returns error.
#[test]
fn mihomo_missing_proxies_key_errors() {
    let yaml = "key: value";
    let err = deve_sub_protocol::container::parse_mihomo_yaml(yaml).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingContainerKey("proxies")
    ));
}

/// Multiple proxies in one YAML.
#[test]
fn mihomo_multiple_proxies() {
    let yaml = format!(
        r#"
proxies:
  - name: "Node1"
    type: trojan
    server: a.example.com
    port: 443
    password: "{RESERVED_PASSWORD}"
  - name: "Node2"
    type: ss
    server: b.example.com
    port: 8388
    cipher: aes-256-gcm
    password: "{RESERVED_PASSWORD}"
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].display_name, "Node1");
    assert_eq!(nodes[1].display_name, "Node2");
}
