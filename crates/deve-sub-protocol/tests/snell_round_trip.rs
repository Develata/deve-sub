//! Round-trip golden tests for Snell (PARSE-024, PARSE-025).
//!
//! Covers URI parse→emit, Mihomo YAML parse→emit, and sing-box JSON
//! parse→emit. Xray/V2Ray do not support Snell and are excluded with
//! report (constraint #7); the existing `xray_preserves_unknown_protocol`
//! test in `xray_v2ray.rs` uses `anytls` as its fixture, covering that
//! path.

#![allow(clippy::expect_used)]

use deve_sub_domain::{
    Authentication, ProtocolConfig, ProtocolKind, SnellConfig, SnellObfsMode, SnellVersion,
};

const PSK: &str = "TEST_SNELL_PSK";

// --- PARSE-024: Snell URI ---

/// PARSE-024: Parse a `snell://` URI with version + udp + obfs.
#[test]
fn snell_uri_parse_full_fidelity() {
    let uri = format!(
        "snell://{PSK}@snell.example.com:8443\
         ?version=4\
         &udp=1\
         &reuse=1\
         &obfs=tls\
         &obfs-host=bing.com\
         #Snell-Test"
    );
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");

    assert_eq!(node.protocol, ProtocolKind::Snell);
    assert_eq!(node.display_name, "Snell-Test");
    assert_eq!(node.endpoint.host.uri_host(), "snell.example.com");
    assert_eq!(node.endpoint.port, 8443);

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password auth");
    };
    assert_eq!(password, PSK);

    assert_eq!(node.udp.supported, Some(true));

    let ProtocolConfig::Snell(SnellConfig {
        version,
        reuse,
        obfs,
        v6_mode,
    }) = &node.config
    else {
        panic!("expected Snell config");
    };
    assert_eq!(*version, SnellVersion::V4);
    assert_eq!(*reuse, Some(true));
    let obfs = obfs.as_ref().expect("obfs");
    assert_eq!(obfs.mode, SnellObfsMode::Tls);
    assert_eq!(obfs.host.as_deref(), Some("bing.com"));
    assert!(v6_mode.is_none());

    // obfs=tls → node.tls must be Some with enabled=true and server_name=obfs-host.
    let tls = node.tls.as_ref().expect("tls");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("bing.com"));
}

/// PARSE-024: Parse → emit → parse yields semantic equality.
#[test]
fn snell_uri_round_trip_semantic() {
    let uri = format!(
        "snell://{PSK}@snell.example.com:8443\
         ?version=4\
         &udp=1\
         &obfs=http\
         &obfs-host=example.com\
         #Snell-RT"
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
    assert_eq!(parsed1.udp, parsed2.udp);
}

/// PARSE-024: Missing psk returns error.
#[test]
fn snell_uri_missing_psk_errors() {
    let uri = "snell://@snell.example.com:8443?version=4#NoPsk";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("psk")
    ));
}

/// PARSE-024: Missing version returns error.
#[test]
fn snell_uri_missing_version_errors() {
    let uri = format!("snell://{PSK}@snell.example.com:8443#NoVer");
    let err = deve_sub_protocol::parse_uri(&uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("version")
    ));
}

/// PARSE-024: Missing port returns error.
#[test]
fn snell_uri_missing_port_errors() {
    let uri = format!("snell://{PSK}@snell.example.com?version=4#NoPort");
    let err = deve_sub_protocol::parse_uri(&uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("port")
    ));
}

/// PARSE-024: `obfs=http` does not set node.tls (Snell has no TLS by default).
#[test]
fn snell_uri_obfs_http_no_tls() {
    let uri = format!("snell://{PSK}@snell.example.com:8443?version=4&obfs=http&obfs-host=h.com#H");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    assert!(node.tls.is_none());
    let ProtocolConfig::Snell(cfg) = &node.config else {
        panic!("expected Snell");
    };
    let obfs = cfg.obfs.as_ref().expect("obfs");
    assert_eq!(obfs.mode, SnellObfsMode::Http);
}

/// PARSE-024: No obfs → no TLS.
#[test]
fn snell_uri_no_obfs_no_tls() {
    let uri = format!("snell://{PSK}@snell.example.com:8443?version=3#Plain");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    assert!(node.tls.is_none());
    let ProtocolConfig::Snell(cfg) = &node.config else {
        panic!("expected Snell");
    };
    assert!(cfg.obfs.is_none());
    assert_eq!(cfg.version, SnellVersion::V3);
}

/// PARSE-024: Invalid version returns error.
#[test]
fn snell_uri_invalid_version_errors() {
    let uri = format!("snell://{PSK}@snell.example.com:8443?version=7#Bad");
    let err = deve_sub_protocol::parse_uri(&uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::InvalidField {
            field: "version",
            ..
        }
    ));
}

/// PARSE-024: psk query param fallback when userinfo is empty.
#[test]
fn snell_uri_psk_query_fallback() {
    let uri = format!("snell://snell.example.com:8443?psk={PSK}&version=4#Fallback");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password");
    };
    assert_eq!(password, PSK);
}

// --- PARSE-025: Mihomo YAML ---

/// PARSE-025: Parse a Mihomo Snell v4 entry with obfs-opts.
#[test]
fn mihomo_snell_v4_full_fidelity() {
    let yaml = format!(
        r#"
proxies:
  - name: "Snell-Mihomo"
    type: snell
    server: snell.example.com
    port: 8443
    psk: "{PSK}"
    version: 4
    udp: true
    reuse: true
    obfs-opts:
      mode: tls
      host: bing.com
"#
    );

    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Snell);
    assert_eq!(node.display_name, "Snell-Mihomo");
    assert_eq!(node.endpoint.host.uri_host(), "snell.example.com");
    assert_eq!(node.endpoint.port, 8443);

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password");
    };
    assert_eq!(password, PSK);

    assert_eq!(node.udp.supported, Some(true));

    let ProtocolConfig::Snell(cfg) = &node.config else {
        panic!("expected Snell config");
    };
    assert_eq!(cfg.version, SnellVersion::V4);
    assert_eq!(cfg.reuse, Some(true));
    let obfs = cfg.obfs.as_ref().expect("obfs");
    assert_eq!(obfs.mode, SnellObfsMode::Tls);
    assert_eq!(obfs.host.as_deref(), Some("bing.com"));

    let tls = node.tls.as_ref().expect("obfs=tls → tls");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("bing.com"));
}

/// PARSE-025: Mihomo YAML parse → emit → parse yields semantic equality.
#[test]
fn mihomo_snell_round_trip_semantic() {
    let yaml = format!(
        r#"
proxies:
  - name: "Snell-RT"
    type: snell
    server: snell.example.com
    port: 8443
    psk: "{PSK}"
    version: 4
    udp: true
    reuse: true
    obfs-opts:
      mode: http
      host: h.com
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
    assert_eq!(parsed1[0].udp, parsed2[0].udp);
}

/// PARSE-025: Mihomo Snell without version defaults to V1.
#[test]
fn mihomo_snell_default_version_v1() {
    let yaml = format!(
        r#"
proxies:
  - name: "Snell-V1"
    type: snell
    server: snell.example.com
    port: 8443
    psk: "{PSK}"
"#
    );
    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    let ProtocolConfig::Snell(cfg) = &nodes[0].config else {
        panic!("expected Snell");
    };
    assert_eq!(cfg.version, SnellVersion::V1);
    assert!(cfg.obfs.is_none());
    assert!(nodes[0].tls.is_none());
}

/// PARSE-025: Mihomo Snell v3 with no obfs → no TLS.
#[test]
fn mihomo_snell_v3_no_obfs_no_tls() {
    let yaml = format!(
        r#"
proxies:
  - name: "Snell-V3"
    type: snell
    server: snell.example.com
    port: 8443
    psk: "{PSK}"
    version: 3
"#
    );
    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert!(nodes[0].tls.is_none());
}

// --- PARSE-025: sing-box JSON ---

/// PARSE-025: Parse a sing-box Snell v4 outbound with obfs.
#[test]
fn singbox_snell_v4_full_fidelity() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "snell",
      "tag": "Snell-Singbox",
      "server": "snell.example.com",
      "server_port": 8443,
      "psk": "{PSK}",
      "version": 4,
      "reuse": true,
      "obfs_mode": "tls",
      "obfs_host": "bing.com",
      "userkey": "TEST_USERKEY",
      "tls": {{
        "enabled": true,
        "server_name": "bing.com"
      }}
    }}
  ]
}}"#
    );

    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);

    let node = &nodes[0];
    assert_eq!(node.protocol, ProtocolKind::Snell);
    assert_eq!(node.display_name, "Snell-Singbox");
    assert_eq!(node.endpoint.host.uri_host(), "snell.example.com");
    assert_eq!(node.endpoint.port, 8443);

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password");
    };
    assert_eq!(password, PSK);

    let ProtocolConfig::Snell(cfg) = &node.config else {
        panic!("expected Snell config");
    };
    assert_eq!(cfg.version, SnellVersion::V4);
    assert_eq!(cfg.reuse, Some(true));
    let obfs = cfg.obfs.as_ref().expect("obfs");
    assert_eq!(obfs.mode, SnellObfsMode::Tls);
    assert_eq!(obfs.host.as_deref(), Some("bing.com"));

    let tls = node.tls.as_ref().expect("tls");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("bing.com"));

    // userkey preserved in extras.
    let userkey = node.extras.get("snell_userkey").and_then(|v| v.as_str());
    assert_eq!(userkey, Some("TEST_USERKEY"));
}

/// PARSE-025: sing-box JSON parse → emit → parse yields semantic equality.
#[test]
fn singbox_snell_round_trip_semantic() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "snell",
      "tag": "Snell-RT",
      "server": "snell.example.com",
      "server_port": 8443,
      "psk": "{PSK}",
      "version": 4,
      "reuse": true,
      "obfs_mode": "http",
      "obfs_host": "h.com",
      "userkey": "UK"
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
    assert_eq!(parsed1[0].extras, parsed2[0].extras);
}

/// PARSE-025: sing-box Snell v6 with mode.
#[test]
fn singbox_snell_v6_mode() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "snell",
      "tag": "Snell-V6",
      "server": "snell.example.com",
      "server_port": 8443,
      "psk": "{PSK}",
      "version": 6,
      "mode": "unshaped"
    }}
  ]
}}"#
    );
    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    let ProtocolConfig::Snell(cfg) = &nodes[0].config else {
        panic!("expected Snell");
    };
    assert_eq!(cfg.version, SnellVersion::V6);
    assert_eq!(cfg.v6_mode, Some(deve_sub_domain::SnellV6Mode::Unshaped));
    assert!(cfg.obfs.is_none());
    assert!(nodes[0].tls.is_none());
}

/// PARSE-025: sing-box Snell v6 default mode.
#[test]
fn singbox_snell_v6_default_mode() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "snell",
      "tag": "Snell-V6-Def",
      "server": "snell.example.com",
      "server_port": 8443,
      "psk": "{PSK}",
      "version": 6
    }}
  ]
}}"#
    );
    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    let ProtocolConfig::Snell(cfg) = &nodes[0].config else {
        panic!("expected Snell");
    };
    assert_eq!(cfg.v6_mode, Some(deve_sub_domain::SnellV6Mode::Default));
}

/// PARSE-025: sing-box rejects Snell v3 (only v4/v6 supported).
#[test]
fn singbox_snell_v3_rejected() {
    let json = format!(
        r#"{{
  "outbounds": [
    {{
      "type": "snell",
      "tag": "Snell-V3-Bad",
      "server": "snell.example.com",
      "server_port": 8443,
      "psk": "{PSK}",
      "version": 3
    }}
  ]
}}"#
    );
    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);
    // v3 surfaces as Unsupported (sing-box outbound only accepts v4/v6).
    assert!(matches!(nodes[0].config, ProtocolConfig::Unsupported(_)));
}

// --- PARSE-025: Compatibility layer version filtering ---

/// PARSE-025: sing-box compatibility check excludes Snell v1/v2/v3/v5.
#[test]
fn compat_singbox_excludes_snell_v3() {
    use deve_sub_compatibility::{ProfileKind, capability_for, check_node};

    let node = snell_node(SnellVersion::V3);
    let cap = capability_for(ProfileKind::SingBox);
    let err = check_node(&node, &cap).expect_err("sing-box should reject snell v3");
    use deve_sub_compatibility::CompatibilityReason;
    assert!(matches!(
        err,
        CompatibilityReason::UnsupportedProtocolVersion {
            protocol: "snell",
            version: 3,
            ..
        }
    ));
}

/// PARSE-025: sing-box compatibility check accepts Snell v4.
#[test]
fn compat_singbox_accepts_snell_v4() {
    use deve_sub_compatibility::{ProfileKind, capability_for, check_node};

    let node = snell_node(SnellVersion::V4);
    let cap = capability_for(ProfileKind::SingBox);
    check_node(&node, &cap).expect("sing-box should accept snell v4");
}

/// PARSE-025: sing-box compatibility check accepts Snell v6.
#[test]
fn compat_singbox_accepts_snell_v6() {
    use deve_sub_compatibility::{ProfileKind, capability_for, check_node};

    let node = snell_node(SnellVersion::V6);
    let cap = capability_for(ProfileKind::SingBox);
    check_node(&node, &cap).expect("sing-box should accept snell v6");
}

/// PARSE-025: mihomo compatibility check accepts all Snell versions.
#[test]
fn compat_mihomo_accepts_snell_v1_through_v5() {
    use deve_sub_compatibility::{ProfileKind, capability_for, check_node};

    let cap = capability_for(ProfileKind::Mihomo);
    for v in [
        SnellVersion::V1,
        SnellVersion::V2,
        SnellVersion::V3,
        SnellVersion::V4,
        SnellVersion::V5,
        SnellVersion::V6,
    ] {
        let node = snell_node(v);
        check_node(&node, &cap).expect("mihomo should accept snell v{?}");
    }
}

/// PARSE-025: Xray compatibility check excludes all Snell versions.
#[test]
fn compat_xray_excludes_snell() {
    use deve_sub_compatibility::{ProfileKind, capability_for, check_node};

    let node = snell_node(SnellVersion::V4);
    let cap = capability_for(ProfileKind::Xray);
    let err = check_node(&node, &cap).expect_err("xray should reject snell");
    use deve_sub_compatibility::CompatibilityReason;
    assert!(matches!(err, CompatibilityReason::UnsupportedProtocol(_)));
}

fn snell_node(version: SnellVersion) -> deve_sub_domain::Node {
    use deve_sub_domain::{
        Endpoint, Host, Node, NodeSource, RegionAssignment, RegionMethod, UdpCapability,
    };
    use deve_sub_kernel::{NodeId, Timestamp};

    Node {
        id: NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAAAA").expect("ulid"),
        display_name: "test".to_owned(),
        protocol: ProtocolKind::Snell,
        config: ProtocolConfig::Snell(SnellConfig {
            version,
            reuse: None,
            obfs: None,
            v6_mode: None,
        }),
        endpoint: Endpoint {
            host: Host::Domain(deve_sub_domain::DomainName::new("example.com".to_owned())),
            port: 8443,
        },
        authentication: Authentication::Password {
            password: PSK.to_owned(),
        },
        transport: None,
        tls: None,
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
