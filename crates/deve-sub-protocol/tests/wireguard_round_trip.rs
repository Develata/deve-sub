//! Round-trip golden tests for WireGuard (PARSE-019, PARSE-020, PARSE-021).
//!
//! Covers URI parse→emit, Mihomo YAML parse→emit, and sing-box JSON
//! parse→emit. Xray WireGuard is covered by a parse-only field-fidelity
//! test because Xray uses a peer-embedded `endpoint` form that does not
//! round-trip back to the canonical `Node.endpoint` without loss.

#![allow(clippy::expect_used)]

use deve_sub_domain::{Authentication, ProtocolConfig, ProtocolKind, WireGuardConfig};

const PRIVATE_KEY: &str = "TEST_PRIVATE_KEY_BASE64";
const PUBLIC_KEY: &str = "TEST_PUBLIC_KEY_BASE64";
const PSK: &str = "TEST_PSK_BASE64";

// --- PARSE-019: WireGuard URI ---

/// PARSE-019: Parse a `wireguard://` URI with all optional fields.
#[test]
fn wireguard_uri_parse_full_fidelity() {
    let uri = format!(
        "wireguard://{PRIVATE_KEY}@wg.example.com:51820\
         ?publickey={PUBLIC_KEY}\
         &address=10.0.0.2/32,fd00::2/128\
         &presharedkey={PSK}\
         &reserved=10,20,30\
         &mtu=1280\
         &keepalive=25\
         #WG-Test"
    );
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");

    assert_eq!(node.protocol, ProtocolKind::WireGuard);
    assert_eq!(node.display_name, "WG-Test");
    assert_eq!(node.endpoint.host.uri_host(), "wg.example.com");
    assert_eq!(node.endpoint.port, 51820);
    assert!(matches!(node.authentication, Authentication::None));

    let ProtocolConfig::WireGuard(WireGuardConfig {
        private_key,
        address,
        peers,
        mtu,
        workers: _,
        dns: _,
    }) = &node.config
    else {
        panic!("expected WireGuard config");
    };
    assert_eq!(private_key, PRIVATE_KEY);
    assert_eq!(
        address,
        &vec!["10.0.0.2/32".to_owned(), "fd00::2/128".to_owned()]
    );
    assert_eq!(*mtu, Some(1280));

    let peer = &peers[0];
    assert_eq!(peer.public_key, PUBLIC_KEY);
    assert_eq!(peer.pre_shared_key.as_deref(), Some(PSK));
    assert_eq!(peer.reserved, Some([10, 20, 30]));
    assert_eq!(
        peer.persistent_keepalive.map(|d| d.whole_seconds()),
        Some(25)
    );
}

/// PARSE-019: Missing required `publickey` returns error.
#[test]
fn wireguard_uri_missing_publickey_errors() {
    let uri = format!("wireguard://{PRIVATE_KEY}@wg.example.com:51820?address=10.0.0.2/32#NoPK");
    let err = deve_sub_protocol::parse_uri(&uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("publickey")
    ));
}

/// PARSE-019: Missing required `address` returns error.
#[test]
fn wireguard_uri_missing_address_errors() {
    let uri =
        format!("wireguard://{PRIVATE_KEY}@wg.example.com:51820?publickey={PUBLIC_KEY}#NoAddr");
    let err = deve_sub_protocol::parse_uri(&uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("address")
    ));
}

/// PARSE-019: Parse → emit → parse yields semantic equality.
#[test]
fn wireguard_uri_round_trip_semantic() {
    let uri = format!(
        "wireguard://{PRIVATE_KEY}@wg.example.com:51820\
         ?publickey={PUBLIC_KEY}\
         &address=10.0.0.2/32\
         &mtu=1280\
         #WG-RT"
    );
    let parsed1 = deve_sub_protocol::parse_uri(&uri).expect("parse 1");
    let emitted = deve_sub_emitter::emit_uri(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::parse_uri(&emitted).expect("parse 2");

    assert_eq!(parsed1.protocol, parsed2.protocol);
    assert_eq!(parsed1.config, parsed2.config);
    assert_eq!(parsed1.endpoint, parsed2.endpoint);
    assert_eq!(parsed1.authentication, parsed2.authentication);
    assert_eq!(parsed1.display_name, parsed2.display_name);
}

/// PARSE-019: Default `allowedips` is `["0.0.0.0/0", "::/0"]` when absent.
#[test]
fn wireguard_uri_default_allowed_ips() {
    let uri = format!(
        "wireguard://{PRIVATE_KEY}@wg.example.com:51820\
         ?publickey={PUBLIC_KEY}\
         &address=10.0.0.2/32\
         #WG-Default-IPs"
    );
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    let ProtocolConfig::WireGuard(cfg) = &node.config else {
        panic!("expected WireGuard");
    };
    assert_eq!(cfg.peers[0].allowed_ips, vec!["0.0.0.0/0", "::/0"]);
}

// --- PARSE-020: Mihomo YAML ---

/// PARSE-020: Parse a Mihomo WireGuard entry with top-level fields.
#[test]
fn mihomo_wireguard_top_level_fields() {
    let yaml = format!(
        r#"
proxies:
  - name: "WG-Mihomo"
    type: wireguard
    server: wg.example.com
    port: 51820
    private-key: "{PRIVATE_KEY}"
    ip: 10.0.0.2/32
    ipv6: fd00::2/128
    public-key: "{PUBLIC_KEY}"
    pre-shared-key: "{PSK}"
    allowed-ips: ["0.0.0.0/0", "::/0"]
    reserved: [10, 20, 30]
    mtu: 1280
    workers: 4
    dns: ["1.1.1.1", "8.8.8.8"]
    udp: true
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::WireGuard);
    assert_eq!(node.display_name, "WG-Mihomo");
    assert_eq!(node.endpoint.host.uri_host(), "wg.example.com");
    assert_eq!(node.endpoint.port, 51820);

    let ProtocolConfig::WireGuard(cfg) = &node.config else {
        panic!("expected WireGuard config");
    };
    assert_eq!(cfg.private_key, PRIVATE_KEY);
    assert_eq!(cfg.address, vec!["10.0.0.2/32", "fd00::2/128"]);
    assert_eq!(cfg.mtu, Some(1280));
    assert_eq!(cfg.workers, Some(4));
    assert_eq!(cfg.dns, vec!["1.1.1.1", "8.8.8.8"]);

    let peer = &cfg.peers[0];
    assert_eq!(peer.public_key, PUBLIC_KEY);
    assert_eq!(peer.pre_shared_key.as_deref(), Some(PSK));
    assert_eq!(peer.reserved, Some([10, 20, 30]));
}

/// PARSE-020: Parse a Mihomo WireGuard entry with `peers` array.
#[test]
fn mihomo_wireguard_peers_array() {
    let yaml = format!(
        r#"
proxies:
  - name: "WG-Peers"
    type: wireguard
    server: wg.example.com
    port: 51820
    private-key: "{PRIVATE_KEY}"
    ip: 10.0.0.2/32
    peers:
      - server: wg.example.com
        port: 51820
        public-key: "{PUBLIC_KEY}"
        pre-shared-key: "{PSK}"
        allowed-ips: ["0.0.0.0/0"]
        reserved: [1, 2, 3]
        persistent-keepalive: 25
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);

    let ProtocolConfig::WireGuard(cfg) = &nodes[0].config else {
        panic!("expected WireGuard");
    };
    let peer = &cfg.peers[0];
    assert_eq!(peer.public_key, PUBLIC_KEY);
    assert_eq!(peer.pre_shared_key.as_deref(), Some(PSK));
    assert_eq!(peer.reserved, Some([1, 2, 3]));
    assert_eq!(
        peer.persistent_keepalive.map(|d| d.whole_seconds()),
        Some(25)
    );
}

/// PARSE-020: Mihomo YAML parse → emit → parse yields semantic equality.
#[test]
fn mihomo_wireguard_round_trip_semantic() {
    let yaml = format!(
        r#"
proxies:
  - name: "WG-RT"
    type: wireguard
    server: wg.example.com
    port: 51820
    private-key: "{PRIVATE_KEY}"
    ip: 10.0.0.2/32
    public-key: "{PUBLIC_KEY}"
    allowed-ips: ["0.0.0.0/0", "::/0"]
    mtu: 1280
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
    assert_eq!(parsed1[0].display_name, parsed2[0].display_name);
}

// --- PARSE-021: sing-box JSON ---

/// PARSE-021: Parse a sing-box WireGuard outbound.
#[test]
fn singbox_wireguard_parse_full_fidelity() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "wireguard",
      "tag": "WG-Singbox",
      "server": "wg.example.com",
      "server_port": 51820,
      "private_key": "{PRIVATE_KEY}",
      "local_address": ["10.0.0.2/32", "fd00::2/128"],
      "mtu": 1280,
      "workers": 4,
      "peers": [
        {{
          "server": "wg.example.com",
          "server_port": 51820,
          "public_key": "{PUBLIC_KEY}",
          "pre_shared_key": "{PSK}",
          "allowed_ips": ["0.0.0.0/0", "::/0"],
          "reserved": [10, 20, 30],
          "persistent_keepalive_interval": 25
        }}
      ]
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::WireGuard);
    assert_eq!(node.display_name, "WG-Singbox");
    assert_eq!(node.endpoint.host.uri_host(), "wg.example.com");
    assert_eq!(node.endpoint.port, 51820);

    let ProtocolConfig::WireGuard(cfg) = &node.config else {
        panic!("expected WireGuard config");
    };
    assert_eq!(cfg.private_key, PRIVATE_KEY);
    assert_eq!(cfg.address, vec!["10.0.0.2/32", "fd00::2/128"]);
    assert_eq!(cfg.mtu, Some(1280));
    assert_eq!(cfg.workers, Some(4));

    let peer = &cfg.peers[0];
    assert_eq!(peer.public_key, PUBLIC_KEY);
    assert_eq!(peer.pre_shared_key.as_deref(), Some(PSK));
    assert_eq!(peer.reserved, Some([10, 20, 30]));
    assert_eq!(
        peer.persistent_keepalive.map(|d| d.whole_seconds()),
        Some(25)
    );
}

/// PARSE-021: sing-box JSON parse → emit → parse yields semantic equality.
#[test]
fn singbox_wireguard_round_trip_semantic() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "wireguard",
      "tag": "WG-RT",
      "server": "wg.example.com",
      "server_port": 51820,
      "private_key": "{PRIVATE_KEY}",
      "local_address": ["10.0.0.2/32"],
      "mtu": 1280,
      "peers": [
        {{
          "server": "wg.example.com",
          "server_port": 51820,
          "public_key": "{PUBLIC_KEY}",
          "allowed_ips": ["0.0.0.0/0"]
        }}
      ]
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
    assert_eq!(parsed1[0].display_name, parsed2[0].display_name);
}

// --- Xray WireGuard (parse-only; Xray puts endpoint inside peers) ---

/// Parse an Xray WireGuard outbound. Xray stores the server endpoint inside
/// each peer (`peers[].endpoint = "host:port"`), so this is a field-fidelity
/// check rather than a round-trip against the canonical `Node.endpoint`.
#[test]
fn xray_wireguard_parse_field_fidelity() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "protocol": "wireguard",
      "tag": "WG-Xray",
      "settings": {{
        "secretKey": "{PRIVATE_KEY}",
        "address": ["10.0.0.2/32"],
        "mtu": 1280,
        "reserved": [10, 20, 30],
        "peers": [
          {{
            "publicKey": "{PUBLIC_KEY}",
            "endpoint": "wg.example.com:51820",
            "preSharedKey": "{PSK}",
            "allowedIPs": ["0.0.0.0/0"],
            "keepAlive": 25
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
    assert_eq!(node.protocol, ProtocolKind::WireGuard);
    assert_eq!(node.display_name, "WG-Xray");
    assert_eq!(node.endpoint.host.uri_host(), "wg.example.com");
    assert_eq!(node.endpoint.port, 51820);

    let ProtocolConfig::WireGuard(cfg) = &node.config else {
        panic!("expected WireGuard config");
    };
    assert_eq!(cfg.private_key, PRIVATE_KEY);
    assert_eq!(cfg.address, vec!["10.0.0.2/32"]);
    assert_eq!(cfg.mtu, Some(1280));

    let peer = &cfg.peers[0];
    assert_eq!(peer.public_key, PUBLIC_KEY);
    assert_eq!(peer.pre_shared_key.as_deref(), Some(PSK));
    assert_eq!(peer.reserved, Some([10, 20, 30]));
    assert_eq!(
        peer.persistent_keepalive.map(|d| d.whole_seconds()),
        Some(25)
    );
}
