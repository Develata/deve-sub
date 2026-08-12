//! Golden tests for Xray and V2Ray JSON parsers (PARSE-007, PARSE-008).

#![allow(clippy::expect_used)]

use deve_sub_domain::{Authentication, ProtocolConfig, ProtocolKind, TransportKind};

const RESERVED_UUID: &str = "00000000-0000-4000-8000-000000000001";
const RESERVED_PASSWORD: &str = "TEST_PASSWORD";

// --- Xray JSON (PARSE-007) ---

/// PARSE-007: Parse a VLESS Reality outbound from Xray JSON with IPv6.
#[test]
fn xray_parses_vless_reality_ipv6() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "vless",
      "tag": "VLESS-IPv6",
      "settings": {{
        "vnext": [
          {{
            "address": "2001:db8::1",
            "port": 443,
            "users": [
              {{
                "id": "{RESERVED_UUID}",
                "encryption": "none",
                "flow": "xtls-rprx-vision"
              }}
            ]
          }}
        ]
      }},
      "streamSettings": {{
        "network": "tcp",
        "security": "reality",
        "realitySettings": {{
          "serverName": "example.com",
          "fingerprint": "chrome",
          "publicKey": "TEST_PUBLIC_KEY",
          "shortId": "01020304"
        }}
      }}
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_xray_json(&json).expect("parse");
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
    let reality = tls.reality.as_ref().expect("reality");
    assert_eq!(reality.public_key, "TEST_PUBLIC_KEY");
    assert_eq!(reality.short_id, "01020304");
    assert_eq!(tls.client_fingerprint.as_deref(), Some("chrome"));
}

/// PARSE-007: Parse a Trojan outbound from Xray JSON.
#[test]
fn xray_parses_trojan() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "trojan",
      "tag": "Trojan-Node",
      "settings": {{
        "servers": [
          {{
            "address": "example.com",
            "port": 443,
            "password": "{RESERVED_PASSWORD}"
          }}
        ]
      }},
      "streamSettings": {{
        "network": "tcp",
        "security": "tls",
        "tlsSettings": {{
          "serverName": "example.com",
          "allowInsecure": false,
          "alpn": ["h2"]
        }}
      }}
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_xray_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Trojan);

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password auth");
    };
    assert_eq!(password, RESERVED_PASSWORD);

    let tls = node.tls.as_ref().expect("tls");
    assert_eq!(tls.server_name.as_deref(), Some("example.com"));
    assert_eq!(tls.skip_cert_verify, Some(false));
    assert_eq!(tls.alpn, vec!["h2"]);
}

/// PARSE-007: Parse a Shadowsocks outbound from Xray JSON.
#[test]
fn xray_parses_shadowsocks() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "shadowsocks",
      "tag": "SS-Node",
      "settings": {{
        "servers": [
          {{
            "address": "ss.example.com",
            "port": 8388,
            "method": "aes-256-gcm",
            "password": "{RESERVED_PASSWORD}"
          }}
        ]
      }}
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_xray_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Shadowsocks);

    let ProtocolConfig::Shadowsocks(cfg) = &node.config else {
        panic!("expected Shadowsocks config");
    };
    assert_eq!(cfg.method, "aes-256-gcm");
}

/// PARSE-007: Parse a VMess outbound with WebSocket transport.
#[test]
fn xray_parses_vmess_ws() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "vmess",
      "tag": "VMess-WS",
      "settings": {{
        "vnext": [
          {{
            "address": "example.com",
            "port": 443,
            "users": [
              {{
                "id": "{RESERVED_UUID}",
                "alterId": 0,
                "security": "auto"
              }}
            ]
          }}
        ]
      }},
      "streamSettings": {{
        "network": "ws",
        "security": "tls",
        "tlsSettings": {{
          "serverName": "example.com"
        }},
        "wsSettings": {{
          "path": "/vmess",
          "headers": {{ "Host": "ws.example.com" }}
        }}
      }}
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_xray_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::VMess);

    let transport = node.transport.as_ref().expect("transport");
    assert_eq!(transport.kind, TransportKind::Ws);
    assert_eq!(transport.path.as_deref(), Some("/vmess"));
    assert_eq!(transport.host.as_deref(), Some("ws.example.com"));

    let ProtocolConfig::VMess(cfg) = &node.config else {
        panic!("expected VMess config");
    };
    assert_eq!(cfg.alter_id, Some(0));
    assert_eq!(cfg.security.as_deref(), Some("auto"));
}

/// PARSE-007: Unknown protocol is preserved as UnsupportedNode.
#[test]
fn xray_preserves_unknown_protocol() {
    let json = r#"{
  "outbounds": [
    {
      "protocol": "anytls",
      "tag": "AnyTLS-Unknown",
      "settings": {}
    }
  ]
}"#;

    let nodes = deve_sub_protocol::container::parse_xray_json(json).expect("parse");
    assert_eq!(nodes.len(), 1);
    assert!(matches!(nodes[0].config, ProtocolConfig::Unsupported(_)));
}

// --- V2Ray JSON (PARSE-008) ---

/// PARSE-008: V2Ray JSON uses the same format as Xray JSON.
#[test]
fn v2ray_parses_trojan() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "trojan",
      "tag": "V2Ray-Trojan",
      "settings": {{
        "servers": [
          {{
            "address": "v2ray.example.com",
            "port": 443,
            "password": "{RESERVED_PASSWORD}"
          }}
        ]
      }},
      "streamSettings": {{
        "network": "tcp",
        "security": "tls",
        "tlsSettings": {{
          "serverName": "v2ray.example.com"
        }}
      }}
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_v2ray_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Trojan);
    assert_eq!(node.display_name, "V2Ray-Trojan");
    assert_eq!(node.endpoint.host.uri_host(), "v2ray.example.com");

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password auth");
    };
    assert_eq!(password, RESERVED_PASSWORD);
}

/// PARSE-008: V2Ray parses VMess.
#[test]
fn v2ray_parses_vmess() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "vmess",
      "tag": "V2Ray-VMess",
      "settings": {{
        "vnext": [
          {{
            "address": "v2.example.com",
            "port": 443,
            "users": [
              {{
                "id": "{RESERVED_UUID}",
                "alterId": 64,
                "security": "aes-128-gcm"
              }}
            ]
          }}
        ]
      }},
      "streamSettings": {{
        "network": "tcp",
        "security": "none"
      }}
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_v2ray_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::VMess);

    let ProtocolConfig::VMess(cfg) = &node.config else {
        panic!("expected VMess config");
    };
    assert_eq!(cfg.alter_id, Some(64));
    assert_eq!(cfg.security.as_deref(), Some("aes-128-gcm"));
    assert!(node.tls.is_none());
}

/// Both Xray and V2Ray parse the same JSON identically.
#[test]
fn xray_v2ray_same_format() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "trojan",
      "tag": "Same",
      "settings": {{
        "servers": [
          {{
            "address": "same.example.com",
            "port": 443,
            "password": "{RESERVED_PASSWORD}"
          }}
        ]
      }},
      "streamSettings": {{
        "network": "tcp",
        "security": "tls",
        "tlsSettings": {{ "serverName": "same.example.com" }}
      }}
    }}
  ]
}}"#
    );

    let xray_nodes = deve_sub_protocol::container::parse_xray_json(&json).expect("xray");
    let v2ray_nodes = deve_sub_protocol::container::parse_v2ray_json(&json).expect("v2ray");

    assert_eq!(xray_nodes.len(), 1);
    assert_eq!(v2ray_nodes.len(), 1);
    assert_eq!(xray_nodes[0].protocol, v2ray_nodes[0].protocol);
    assert_eq!(xray_nodes[0].endpoint, v2ray_nodes[0].endpoint);
    assert_eq!(xray_nodes[0].authentication, v2ray_nodes[0].authentication);
    assert_eq!(xray_nodes[0].tls, v2ray_nodes[0].tls);
}

/// Missing `outbounds` key returns error.
#[test]
fn xray_missing_outbounds_errors() {
    let json = r#"{"key": "value"}"#;
    let err = deve_sub_protocol::container::parse_xray_json(json).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingContainerKey("outbounds")
    ));
}

// --- Round-trip tests (emit → parse → compare) ---

/// Helper: parse JSON → emit → parse emitted JSON → compare protocol,
/// endpoint, authentication, TLS, and transport with the first parse.
fn assert_xray_round_trip(json: &str) {
    let parsed1 = deve_sub_protocol::container::parse_xray_json(json).expect("parse 1");
    assert_eq!(parsed1.len(), 1);
    let emitted = deve_sub_emitter::emit_xray(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::container::parse_xray_json(&emitted).expect("parse 2");
    assert_eq!(parsed2.len(), 1);
    let a = &parsed1[0];
    let b = &parsed2[0];
    assert_eq!(a.protocol, b.protocol, "protocol mismatch");
    assert_eq!(a.endpoint, b.endpoint, "endpoint mismatch");
    assert_eq!(
        a.authentication, b.authentication,
        "authentication mismatch"
    );
    assert_eq!(a.tls, b.tls, "tls mismatch");
    assert_eq!(a.transport, b.transport, "transport mismatch");
    assert_eq!(a.config, b.config, "config mismatch");
}

/// Trojan round-trip: parse → emit → parse produces equivalent node.
#[test]
fn xray_trojan_round_trip() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "trojan",
      "tag": "RT-Trojan",
      "settings": {{
        "servers": [
          {{ "address": "rt.example.com", "port": 443, "password": "{RESERVED_PASSWORD}" }}
        ]
      }},
      "streamSettings": {{
        "network": "tcp",
        "security": "tls",
        "tlsSettings": {{ "serverName": "rt.example.com" }}
      }}
    }}
  ]
}}"#
    );
    assert_xray_round_trip(&json);
}

/// Shadowsocks round-trip: parse → emit → parse produces equivalent node.
#[test]
fn xray_shadowsocks_round_trip() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "shadowsocks",
      "tag": "RT-SS",
      "settings": {{
        "servers": [
          {{ "address": "ss.example.com", "port": 8388, "method": "aes-256-gcm", "password": "{RESERVED_PASSWORD}" }}
        ]
      }}
    }}
  ]
}}"#
    );
    assert_xray_round_trip(&json);
}

/// VMess round-trip: parse → emit → parse produces equivalent node.
#[test]
fn xray_vmess_round_trip() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "vmess",
      "tag": "RT-VMess",
      "settings": {{
        "vnext": [
          {{
            "address": "vmess.example.com",
            "port": 443,
            "users": [{{ "id": "{RESERVED_UUID}", "alterId": 0, "security": "auto" }}]
          }}
        ]
      }},
      "streamSettings": {{ "network": "tcp", "security": "none" }}
    }}
  ]
}}"#
    );
    assert_xray_round_trip(&json);
}

/// VLESS Reality round-trip: parse → emit → parse produces equivalent node.
#[test]
fn xray_vless_reality_round_trip() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "vless",
      "tag": "RT-VLESS",
      "settings": {{
        "vnext": [
          {{
            "address": "vless.example.com",
            "port": 443,
            "users": [{{ "id": "{RESERVED_UUID}", "encryption": "none", "flow": "xtls-rprx-vision" }}]
          }}
        ]
      }},
      "streamSettings": {{
        "network": "tcp",
        "security": "reality",
        "realitySettings": {{
          "serverName": "vless.example.com",
          "fingerprint": "chrome",
          "publicKey": "RT_PUBLIC_KEY",
          "shortId": "01020304"
        }}
      }}
    }}
  ]
}}"#
    );
    assert_xray_round_trip(&json);
}

/// VMess + WebSocket transport round-trip.
#[test]
fn xray_vmess_ws_round_trip() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "vmess",
      "tag": "RT-VMess-WS",
      "settings": {{
        "vnext": [
          {{
            "address": "ws.example.com",
            "port": 443,
            "users": [{{ "id": "{RESERVED_UUID}", "alterId": 0, "security": "auto" }}]
          }}
        ]
      }},
      "streamSettings": {{
        "network": "ws",
        "security": "tls",
        "tlsSettings": {{ "serverName": "ws.example.com" }},
        "wsSettings": {{ "path": "/ws", "headers": {{ "Host": "ws.example.com" }} }}
      }}
    }}
  ]
}}"#
    );
    assert_xray_round_trip(&json);
}
