//! Round-trip golden tests for ShadowTLS (PARSE-026).
//!
//! Covers sing-box JSON parse→emit (standalone `shadowtls` type + detour
//! merge) and mihomo YAML parse→emit (obfs projection under inner
//! protocol). Xray/V2Ray do not support ShadowTLS and are excluded with
//! report (constraint #7).

#![allow(clippy::expect_used)]

use deve_sub_domain::{
    Authentication, Node, ProtocolConfig, ProtocolKind, ShadowTlsConfig, ShadowTlsVersion,
};

const PASSWORD: &str = "TEST_STLS_PASSWORD";
const INNER_PASSWORD: &str = "TEST_INNER_PASSWORD";

// --- PARSE-026: sing-box JSON standalone + detour merge ---

/// PARSE-026: sing-box shadowtls + trojan (detour) merge into 1 Node.
#[test]
fn singbox_shadowtls_trojan_merge() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "shadowtls",
      "tag": "stls-out",
      "server": "stls.example.com",
      "server_port": 443,
      "version": 2,
      "password": "{PASSWORD}",
      "tls": {{
        "enabled": true,
        "server_name": "bing.com"
      }}
    }},
    {{
      "type": "trojan",
      "tag": "Trojan-ShadowTLS",
      "server": "stls.example.com",
      "server_port": 443,
      "password": "{INNER_PASSWORD}",
      "detour": "stls-out"
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    // WHY: shadowtls + inner (detour) merge into 1 node; the shadowtls
    // outbound is absorbed, leaving only the merged ShadowTls node.
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::ShadowTls);
    assert_eq!(node.display_name, "Trojan-ShadowTLS");
    assert_eq!(node.endpoint.host.uri_host(), "stls.example.com");
    assert_eq!(node.endpoint.port, 443);

    let ProtocolConfig::ShadowTls(cfg) = &node.config else {
        panic!("expected ShadowTls config");
    };
    assert_eq!(cfg.version, ShadowTlsVersion::V2);
    assert_eq!(cfg.password.as_deref(), Some(PASSWORD));
    assert_eq!(cfg.inner_protocol, ProtocolKind::Trojan);
    assert!(matches!(
        cfg.inner_config.as_ref(),
        ProtocolConfig::Trojan(_)
    ));

    // Inner protocol's authentication is adopted.
    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password auth from inner trojan");
    };
    assert_eq!(password, INNER_PASSWORD);

    // Camouflage TLS from shadowtls outbound.
    let tls = node.tls.as_ref().expect("tls");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("bing.com"));
}

/// PARSE-026: sing-box parse → emit → parse yields semantic equality.
#[test]
fn singbox_shadowtls_round_trip_semantic() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "shadowtls",
      "tag": "stls-rt",
      "server": "stls.example.com",
      "server_port": 443,
      "version": 3,
      "password": "{PASSWORD}",
      "tls": {{
        "enabled": true,
        "server_name": "cover.com"
      }}
    }},
    {{
      "type": "trojan",
      "tag": "RT-Test",
      "server": "stls.example.com",
      "server_port": 443,
      "password": "{INNER_PASSWORD}",
      "detour": "stls-rt"
    }}
  ]
}}"#
    );
    let parsed1 = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse 1");
    assert_eq!(parsed1.len(), 1);

    let emitted = deve_sub_emitter::emit_singbox(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::container::parse_singbox_json(&emitted).expect("parse 2");
    assert_eq!(parsed2.len(), 1);

    assert_eq!(parsed1[0].protocol, parsed2[0].protocol);
    assert_eq!(parsed1[0].display_name, parsed2[0].display_name);
    assert_eq!(parsed1[0].endpoint, parsed2[0].endpoint);
    assert_eq!(parsed1[0].authentication, parsed2[0].authentication);
    assert_eq!(parsed1[0].tls, parsed2[0].tls);

    let ProtocolConfig::ShadowTls(c1) = &parsed1[0].config else {
        panic!("expected ShadowTls");
    };
    let ProtocolConfig::ShadowTls(c2) = &parsed2[0].config else {
        panic!("expected ShadowTls");
    };
    assert_eq!(c1.version, c2.version);
    assert_eq!(c1.password, c2.password);
    assert_eq!(c1.inner_protocol, c2.inner_protocol);
}

/// PARSE-026: sing-box shadowtls with shadowsocks inner.
#[test]
fn singbox_shadowtls_shadowsocks_inner() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "shadowtls",
      "tag": "stls-ss",
      "server": "stls.example.com",
      "server_port": 443,
      "version": 2,
      "password": "{PASSWORD}",
      "tls": {{
        "enabled": true,
        "server_name": "cover.com"
      }}
    }},
    {{
      "type": "shadowsocks",
      "tag": "SS-ShadowTLS",
      "server": "stls.example.com",
      "server_port": 443,
      "method": "aes-256-gcm",
      "password": "{INNER_PASSWORD}",
      "detour": "stls-ss"
    }}
  ]
}}"#
    );
    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].protocol, ProtocolKind::ShadowTls);
    let ProtocolConfig::ShadowTls(cfg) = &nodes[0].config else {
        panic!("expected ShadowTls");
    };
    assert_eq!(cfg.inner_protocol, ProtocolKind::Shadowsocks);
    assert!(matches!(
        cfg.inner_config.as_ref(),
        ProtocolConfig::Shadowsocks(_)
    ));
}

/// PARSE-026: standalone shadowtls (no detour reference) surfaces as
/// ShadowTls with inner_protocol = Unknown.
#[test]
fn singbox_shadowtls_standalone_no_inner() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "shadowtls",
      "tag": "stls-alone",
      "server": "stls.example.com",
      "server_port": 443,
      "version": 2,
      "password": "{PASSWORD}",
      "tls": {{
        "enabled": true,
        "server_name": "cover.com"
      }}
    }}
  ]
}}"#
    );
    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].protocol, ProtocolKind::ShadowTls);
    let ProtocolConfig::ShadowTls(cfg) = &nodes[0].config else {
        panic!("expected ShadowTls");
    };
    assert!(matches!(cfg.inner_protocol, ProtocolKind::Unknown(_)));
}

// --- PARSE-026: mihomo YAML obfs projection ---

/// PARSE-026: mihomo trojan with shadow-tls-opts → ShadowTls node.
#[test]
fn mihomo_shadowtls_trojan_projection() {
    let yaml = format!(
        r#"
proxies:
  - name: "Trojan-STLS"
    type: trojan
    server: stls.example.com
    port: 443
    password: "{INNER_PASSWORD}"
    sni: bing.com
    shadow-tls-opts:
      version: 2
      password: "{PASSWORD}"
      sni: bing.com
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::ShadowTls);
    assert_eq!(node.display_name, "Trojan-STLS");

    let ProtocolConfig::ShadowTls(cfg) = &node.config else {
        panic!("expected ShadowTls config");
    };
    assert_eq!(cfg.version, ShadowTlsVersion::V2);
    assert_eq!(cfg.password.as_deref(), Some(PASSWORD));
    assert_eq!(cfg.inner_protocol, ProtocolKind::Trojan);
    assert!(matches!(
        cfg.inner_config.as_ref(),
        ProtocolConfig::Trojan(_)
    ));

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password auth");
    };
    assert_eq!(password, INNER_PASSWORD);

    let tls = node.tls.as_ref().expect("tls");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("bing.com"));
}

/// PARSE-026: mihomo trojan + shadow-tls-opts parse → emit → parse equality.
#[test]
fn mihomo_shadowtls_trojan_round_trip_semantic() {
    let yaml = format!(
        r#"
proxies:
  - name: "RT-STLS"
    type: trojan
    server: stls.example.com
    port: 443
    password: "{INNER_PASSWORD}"
    sni: cover.com
    shadow-tls-opts:
      version: 3
      password: "{PASSWORD}"
      sni: cover.com
"#
    );
    let parsed1 = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse 1");
    assert_eq!(parsed1.len(), 1);
    assert_eq!(parsed1[0].protocol, ProtocolKind::ShadowTls);

    let emitted = deve_sub_emitter::emit_mihomo(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::container::parse_mihomo_yaml(&emitted).expect("parse 2");
    assert_eq!(parsed2.len(), 1);

    assert_eq!(parsed1[0].protocol, parsed2[0].protocol);
    assert_eq!(parsed1[0].display_name, parsed2[0].display_name);
    assert_eq!(parsed1[0].endpoint, parsed2[0].endpoint);
    assert_eq!(parsed1[0].authentication, parsed2[0].authentication);
    assert_eq!(parsed1[0].tls, parsed2[0].tls);

    let ProtocolConfig::ShadowTls(c1) = &parsed1[0].config else {
        panic!("expected ShadowTls");
    };
    let ProtocolConfig::ShadowTls(c2) = &parsed2[0].config else {
        panic!("expected ShadowTls");
    };
    assert_eq!(c1.version, c2.version);
    assert_eq!(c1.password, c2.password);
    assert_eq!(c1.inner_protocol, c2.inner_protocol);
}

/// PARSE-026: mihomo SS + plugin: shadow-tls projection.
#[test]
fn mihomo_shadowtls_ss_plugin_projection() {
    let yaml = format!(
        r#"
proxies:
  - name: "SS-STLS"
    type: ss
    server: stls.example.com
    port: 443
    cipher: aes-256-gcm
    password: "{INNER_PASSWORD}"
    plugin: shadow-tls
    plugin-opts:
      version: 2
      password: "{PASSWORD}"
      host: bing.com
"#
    );
    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].protocol, ProtocolKind::ShadowTls);
    let ProtocolConfig::ShadowTls(cfg) = &nodes[0].config else {
        panic!("expected ShadowTls");
    };
    assert_eq!(cfg.inner_protocol, ProtocolKind::Shadowsocks);
    assert!(matches!(
        cfg.inner_config.as_ref(),
        ProtocolConfig::Shadowsocks(_)
    ));
}

/// PARSE-026: mihomo SS + plugin: shadow-tls round-trip.
#[test]
fn mihomo_shadowtls_ss_round_trip_semantic() {
    let yaml = format!(
        r#"
proxies:
  - name: "SS-RT"
    type: ss
    server: stls.example.com
    port: 443
    cipher: aes-256-gcm
    password: "{INNER_PASSWORD}"
    plugin: shadow-tls
    plugin-opts:
      version: 2
      password: "{PASSWORD}"
      host: cover.com
"#
    );
    let parsed1 = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse 1");
    assert_eq!(parsed1[0].protocol, ProtocolKind::ShadowTls);
    let emitted = deve_sub_emitter::emit_mihomo(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::container::parse_mihomo_yaml(&emitted).expect("parse 2");
    assert_eq!(parsed2.len(), 1);
    assert_eq!(parsed1[0].protocol, parsed2[0].protocol);
    assert_eq!(parsed1[0].authentication, parsed2[0].authentication);
    let ProtocolConfig::ShadowTls(c1) = &parsed1[0].config else {
        panic!("expected ShadowTls");
    };
    let ProtocolConfig::ShadowTls(c2) = &parsed2[0].config else {
        panic!("expected ShadowTls");
    };
    assert_eq!(c1.version, c2.version);
    assert_eq!(c1.password, c2.password);
    assert_eq!(c1.inner_protocol, c2.inner_protocol);
}

// --- PARSE-026: URI round-trip (wrapper only — no inner protocol) ---

/// PARSE-026: shadow-tls:// URI parse — basic field verification.
#[test]
fn shadowtls_uri_parse_basic() {
    let uri =
        format!("shadow-tls://{PASSWORD}@stls.example.com:443?version=2&sni=bing.com#STLS-URI");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    assert_eq!(node.protocol, ProtocolKind::ShadowTls);
    assert_eq!(node.display_name, "STLS-URI");
    assert_eq!(node.endpoint.host.uri_host(), "stls.example.com");
    assert_eq!(node.endpoint.port, 443);

    let ProtocolConfig::ShadowTls(cfg) = &node.config else {
        panic!("expected ShadowTls");
    };
    assert_eq!(cfg.version, ShadowTlsVersion::V2);
    assert_eq!(cfg.password.as_deref(), Some(PASSWORD));
    // WHY: URI does not carry inner protocol — placeholder is Unknown.
    assert!(matches!(cfg.inner_protocol, ProtocolKind::Unknown(_)));
    // WHY: node.authentication is None for URI-parsed ShadowTls (no inner
    // protocol to authenticate). Wrapper password lives in cfg.password.
    assert!(matches!(node.authentication, Authentication::None));

    let tls = node.tls.as_ref().expect("tls");
    assert_eq!(tls.server_name.as_deref(), Some("bing.com"));
}

/// PARSE-026: shadow-tls:// URI parse → emit → parse round-trip.
#[test]
fn shadowtls_uri_round_trip() {
    let uri = format!("shadow-tls://{PASSWORD}@stls.example.com:443?version=3&sni=cover.com#RT");
    let parsed1 = deve_sub_protocol::parse_uri(&uri).expect("parse 1");
    let emitted = deve_sub_emitter::emit_uri(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::parse_uri(&emitted).expect("parse 2");
    assert_eq!(parsed1.protocol, parsed2.protocol);
    assert_eq!(parsed1.endpoint, parsed2.endpoint);
    assert_eq!(parsed1.tls, parsed2.tls);
    assert_eq!(parsed1.display_name, parsed2.display_name);
    assert_eq!(parsed1.config, parsed2.config);
}

// --- PARSE-026: Compatibility layer ---

/// PARSE-026: Xray compatibility check excludes ShadowTLS.
#[test]
fn compat_xray_excludes_shadowtls() {
    use deve_sub_compatibility::{CompatibilityReason, ProfileKind, capability_for, check_node};

    let node = shadowtls_node();
    let cap = capability_for(ProfileKind::Xray);
    let err = check_node(&node, &cap).expect_err("xray should reject shadowtls");
    assert!(matches!(err, CompatibilityReason::UnsupportedProtocol(_)));
}

/// PARSE-026: Mihomo compatibility check accepts ShadowTLS.
#[test]
fn compat_mihomo_accepts_shadowtls() {
    use deve_sub_compatibility::{ProfileKind, capability_for, check_node};

    let node = shadowtls_node();
    let cap = capability_for(ProfileKind::Mihomo);
    check_node(&node, &cap).expect("mihomo should accept shadowtls");
}

/// PARSE-026: sing-box compatibility check accepts ShadowTLS.
#[test]
fn compat_singbox_accepts_shadowtls() {
    use deve_sub_compatibility::{ProfileKind, capability_for, check_node};

    let node = shadowtls_node();
    let cap = capability_for(ProfileKind::SingBox);
    check_node(&node, &cap).expect("sing-box should accept shadowtls");
}

fn shadowtls_node() -> Node {
    use deve_sub_domain::{
        DomainName, Endpoint, Host, NodeSource, RegionAssignment, RegionMethod, TlsConfig,
        UdpCapability,
    };
    use deve_sub_kernel::{NodeId, Timestamp};

    Node {
        id: NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAAAA").expect("ulid"),
        display_name: "test".to_owned(),
        protocol: ProtocolKind::ShadowTls,
        config: ProtocolConfig::ShadowTls(ShadowTlsConfig {
            version: ShadowTlsVersion::V2,
            password: Some("pw".to_owned()),
            inner_protocol: ProtocolKind::Trojan,
            inner_config: Box::new(ProtocolConfig::Trojan(deve_sub_domain::TrojanConfig {
                packet_encoding: None,
            })),
        }),
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new("example.com".to_owned())),
            port: 443,
        },
        authentication: Authentication::Password {
            password: "inner-pw".to_owned(),
        },
        transport: None,
        tls: Some(TlsConfig {
            enabled: true,
            server_name: Some("cover.com".to_owned()),
            skip_cert_verify: None,
            alpn: vec![],
            client_fingerprint: None,
            certificate_pins: vec![],
            reality: None,
        }),
        udp: UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "test".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::from_unix_ms(0).expect("ts"),
        },
        tags: vec![],
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: None,
        },
        extras: std::collections::BTreeMap::new(),
    }
}
