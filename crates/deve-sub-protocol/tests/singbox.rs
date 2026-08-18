//! Golden tests for sing-box JSON parser (PARSE-006).

#![allow(clippy::expect_used)]

use deve_sub_domain::{Authentication, ProtocolConfig, ProtocolKind, TransportKind, UdpRelayMode};

const RESERVED_UUID: &str = "00000000-0000-4000-8000-000000000001";
const RESERVED_PASSWORD: &str = "TEST_PASSWORD";

/// PARSE-006: Parse a VLESS Reality outbound with IPv6.
#[test]
fn singbox_parses_vless_reality_ipv6() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "vless",
      "tag": "VLESS-IPv6",
      "server": "2001:db8::1",
      "server_port": 443,
      "uuid": "{RESERVED_UUID}",
      "flow": "xtls-rprx-vision",
      "tls": {{
        "enabled": true,
        "server_name": "example.com",
        "insecure": false,
        "reality": {{
          "enabled": true,
          "public_key": "TEST_PUBLIC_KEY",
          "short_id": "01020304"
        }},
        "utls": {{
          "enabled": true,
          "fingerprint": "chrome"
        }}
      }}
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
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
    let reality = tls.reality.as_ref().expect("reality");
    assert_eq!(reality.public_key, "TEST_PUBLIC_KEY");
    assert_eq!(reality.short_id, "01020304");
}

/// PARSE-006: Parse a Trojan outbound with WebSocket transport.
#[test]
fn singbox_parses_trojan_ws() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "trojan",
      "tag": "Trojan-WS",
      "server": "example.com",
      "server_port": 443,
      "password": "{RESERVED_PASSWORD}",
      "tls": {{
        "enabled": true,
        "server_name": "example.com",
        "alpn": ["h2", "http/1.1"]
      }},
      "transport": {{
        "type": "ws",
        "path": "/ws",
        "headers": {{ "Host": "ws.example.com" }}
      }}
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Trojan);

    let transport = node.transport.as_ref().expect("transport");
    assert_eq!(transport.kind, TransportKind::Ws);
    assert_eq!(transport.path.as_deref(), Some("/ws"));
    assert_eq!(transport.host.as_deref(), Some("ws.example.com"));

    let tls = node.tls.as_ref().expect("tls");
    assert_eq!(tls.alpn, vec!["h2", "http/1.1"]);
}

/// PARSE-006: Parse a Shadowsocks outbound.
#[test]
fn singbox_parses_shadowsocks() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "shadowsocks",
      "tag": "SS-Node",
      "server": "ss.example.com",
      "server_port": 8388,
      "method": "aes-256-gcm",
      "password": "{RESERVED_PASSWORD}"
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Shadowsocks);

    let ProtocolConfig::Shadowsocks(cfg) = &node.config else {
        panic!("expected Shadowsocks config");
    };
    assert_eq!(cfg.method, "aes-256-gcm");
}

/// PARSE-006: Parse a Hysteria2 outbound.
#[test]
fn singbox_parses_hysteria2() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "hysteria2",
      "tag": "Hy2-Node",
      "server": "hy2.example.com",
      "server_port": 443,
      "password": "{RESERVED_PASSWORD}",
      "tls": {{
        "enabled": true,
        "server_name": "hy2.example.com"
      }},
      "obfs": "salamander",
      "obfs_password": "obfs-pwd"
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Hysteria2);

    let obfs = node.obfuscation.as_ref().expect("obfs");
    assert_eq!(obfs.kind, "salamander");
    assert_eq!(obfs.password.as_deref(), Some("obfs-pwd"));
}

/// PARSE-006: Parse a TUIC v5 outbound.
#[test]
fn singbox_parses_tuic_v5() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "tuic",
      "tag": "TUIC-Node",
      "server": "tuic.example.com",
      "server_port": 443,
      "uuid": "{RESERVED_UUID}",
      "password": "{RESERVED_PASSWORD}",
      "congestion_control": "bbr",
      "udp_relay_mode": "native",
      "tls": {{
        "enabled": true,
        "server_name": "tuic.example.com",
        "alpn": ["h3"]
      }}
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::TuicV5);

    let ProtocolConfig::TuicV5(cfg) = &node.config else {
        panic!("expected TuicV5 config");
    };
    assert_eq!(cfg.udp_relay_mode, Some(UdpRelayMode::Native));
}

/// PARSE-006: sing-box internal outbounds (direct, block, dns) are preserved
/// as UnsupportedNode, not treated as proxy nodes.
#[test]
fn singbox_internal_outbounds_preserved() {
    let json = r#"{
  "outbounds": [
    {"type": "direct", "tag": "direct"},
    {"type": "block", "tag": "block"},
    {"type": "dns", "tag": "dns-out"},
    {"type": "selector", "tag": "proxy", "outbounds": ["direct"]}
  ]
}"#;

    let nodes = deve_sub_protocol::container::parse_singbox_json(json).expect("parse");
    assert_eq!(nodes.len(), 4);
    for node in &nodes {
        assert!(matches!(node.config, ProtocolConfig::Unsupported(_)));
    }
}

/// Missing `outbounds` key returns error.
#[test]
fn singbox_missing_outbounds_errors() {
    let json = r#"{"key": "value"}"#;
    let err = deve_sub_protocol::container::parse_singbox_json(json).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingContainerKey("outbounds")
    ));
}

/// Invalid JSON returns error.
#[test]
fn singbox_invalid_json_errors() {
    let err =
        deve_sub_protocol::container::parse_singbox_json("not json").expect_err("should fail");
    assert!(matches!(err, deve_sub_protocol::ParseError::InvalidJson(_)));
}

/// P3-3: sing-box parser must preserve plugin_opts for Shadowsocks.
#[test]
fn singbox_parses_shadowsocks_plugin_opts() {
    let json = r#"{
  "outbounds": [
    {
      "type": "shadowsocks",
      "tag": "SS-Plugin",
      "server": "ss.example.com",
      "server_port": 8388,
      "method": "aes-256-gcm",
      "password": "TEST_PASSWORD",
      "plugin": "obfs-local",
      "plugin_opts": "obfs=tls;obfs-host=example.com"
    }
  ]
}"#;

    let nodes = deve_sub_protocol::container::parse_singbox_json(json).expect("parse");
    assert_eq!(nodes.len(), 1);
    let ProtocolConfig::Shadowsocks(cfg) = &nodes[0].config else {
        panic!("expected Shadowsocks");
    };
    assert_eq!(cfg.plugin.as_deref(), Some("obfs-local"));
    assert_eq!(
        cfg.plugin_opts.as_deref(),
        Some("obfs=tls;obfs-host=example.com"),
        "plugin_opts must be preserved"
    );
}

/// P3-1: sing-box parser must preserve packet_encoding for VMess.
#[test]
fn singbox_parses_vmess_packet_encoding() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "vmess",
      "tag": "VMess-PE",
      "server": "vmess.example.com",
      "server_port": 443,
      "uuid": "{RESERVED_UUID}",
      "packet_encoding": "xudp"
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);
    let ProtocolConfig::VMess(cfg) = &nodes[0].config else {
        panic!("expected VMess");
    };
    assert_eq!(
        cfg.packet_encoding.as_deref(),
        Some("xudp"),
        "packet_encoding must be preserved"
    );
}
