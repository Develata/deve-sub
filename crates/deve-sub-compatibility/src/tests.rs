use super::*;
use deve_sub_domain::{
    Authentication, DomainName, Endpoint, Host, Node, NodeSource, ProtocolConfig, ProtocolKind,
    RegionAssignment, RegionMethod, SnellConfig, SnellObfs, SnellObfsMode, SnellVersion, Transport,
    TransportKind, TrojanConfig, UdpCapability,
};
use deve_sub_kernel::{NodeId, Timestamp};

fn make_trojan_node(transport: Option<TransportKind>) -> Node {
    Node {
        id: NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAAAA").expect("ulid"),
        display_name: "test".to_owned(),
        protocol: ProtocolKind::Trojan,
        config: ProtocolConfig::Trojan(TrojanConfig {
            packet_encoding: None,
        }),
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new("example.com".to_owned())),
            port: 443,
        },
        authentication: Authentication::Password {
            password: "pw".to_owned(),
        },
        transport: transport.map(|k| Transport {
            kind: k,
            path: None,
            host: None,
            xhttp_mode: None,
        }),
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

#[test]
fn mihomo_supports_trojan_tcp() {
    let node = make_trojan_node(Some(TransportKind::Tcp));
    let cap = capability_for(ProfileKind::Mihomo);
    assert!(check_node(&node, &cap).is_ok());
}

#[test]
fn xray_rejects_hysteria2() {
    let mut node = make_trojan_node(None);
    node.protocol = ProtocolKind::Hysteria2;
    node.config = ProtocolConfig::Hysteria2(deve_sub_domain::Hysteria2Config {
        ports: None,
        hop_interval: None,
        fast_open: None,
        lazy: None,
    });
    let cap = capability_for(ProfileKind::Xray);
    let err = check_node(&node, &cap).expect_err("xray should reject hysteria2");
    assert!(matches!(err, CompatibilityReason::UnsupportedProtocol(_)));
}

#[test]
fn v2ray_rejects_http_upgrade_transport() {
    let node = make_trojan_node(Some(TransportKind::HttpUpgrade));
    let cap = capability_for(ProfileKind::V2Ray);
    let err = check_node(&node, &cap).expect_err("v2ray should reject httpupgrade");
    assert!(matches!(err, CompatibilityReason::UnsupportedTransport(_)));
}

#[test]
fn uri_list_accepts_all_uri_protocols() {
    let node = make_trojan_node(Some(TransportKind::Tcp));
    let cap = capability_for(ProfileKind::UriList);
    assert!(check_node(&node, &cap).is_ok());
}

#[test]
fn unsupported_config_rejected() {
    let mut node = make_trojan_node(None);
    node.config = ProtocolConfig::Unsupported(deve_sub_domain::UnsupportedNode {
        raw: serde_json::Value::Null,
        raw_format: None,
        reason: "test".to_owned(),
    });
    let cap = capability_for(ProfileKind::Mihomo);
    let err = check_node(&node, &cap).expect_err("mihomo should reject unsupported config");
    assert!(matches!(err, CompatibilityReason::UnsupportedConfig));
}

#[test]
fn mihomo_supports_relay_group() {
    let cap = capability_for(ProfileKind::Mihomo);
    assert!(check_group_type(GroupType::Relay, &cap).is_ok());
}

#[test]
fn singbox_rejects_relay_group() {
    let cap = capability_for(ProfileKind::SingBox);
    assert!(check_group_type(GroupType::Relay, &cap).is_err());
}

#[test]
fn uri_list_accepts_any_group_type() {
    let cap = capability_for(ProfileKind::UriList);
    assert!(check_group_type(GroupType::Select, &cap).is_ok());
    assert!(check_group_type(GroupType::Relay, &cap).is_ok());
}

#[test]
fn profile_kind_roundtrip() {
    for p in [
        ProfileKind::Mihomo,
        ProfileKind::SingBox,
        ProfileKind::Xray,
        ProfileKind::V2Ray,
        ProfileKind::Shadowrocket,
        ProfileKind::UriList,
    ] {
        assert_eq!(ProfileKind::from_kebab(p.as_kebab()), Some(p));
    }
    assert!(ProfileKind::from_kebab("unknown").is_none());
}

fn make_snell_node(version: SnellVersion, obfs: Option<SnellObfsMode>) -> Node {
    let mut node = make_trojan_node(None);
    node.protocol = ProtocolKind::Snell;
    node.config = ProtocolConfig::Snell(SnellConfig {
        version,
        reuse: None,
        obfs: obfs.map(|m| SnellObfs {
            mode: m,
            host: None,
            password: None,
            version: None,
            alpn: vec![],
        }),
        v6_mode: None,
    });
    node
}

#[test]
fn singbox_accepts_snell_v4_http_obfs() {
    let node = make_snell_node(SnellVersion::V4, Some(SnellObfsMode::Http));
    let cap = capability_for(ProfileKind::SingBox);
    check_node(&node, &cap).expect("sing-box should accept snell v4 http obfs");
}

#[test]
fn singbox_accepts_snell_v4_tls_obfs() {
    let node = make_snell_node(SnellVersion::V4, Some(SnellObfsMode::Tls));
    let cap = capability_for(ProfileKind::SingBox);
    check_node(&node, &cap).expect("sing-box should accept snell v4 tls obfs");
}

#[test]
fn singbox_rejects_snell_v4_shadowtls_obfs() {
    let node = make_snell_node(SnellVersion::V4, Some(SnellObfsMode::ShadowTls));
    let cap = capability_for(ProfileKind::SingBox);
    let err = check_node(&node, &cap).expect_err("sing-box should reject snell v4 shadow-tls");
    assert!(matches!(
        err,
        CompatibilityReason::UnsupportedObfsMode {
            protocol: "snell",
            profile: "sing-box",
            mode: "shadow-tls",
            supported: "http, tls",
        }
    ));
}

#[test]
fn singbox_rejects_snell_v4_restls_obfs() {
    let node = make_snell_node(SnellVersion::V4, Some(SnellObfsMode::Restls));
    let cap = capability_for(ProfileKind::SingBox);
    let err = check_node(&node, &cap).expect_err("sing-box should reject snell v4 restls");
    assert!(matches!(
        err,
        CompatibilityReason::UnsupportedObfsMode {
            protocol: "snell",
            profile: "sing-box",
            mode: "restls",
            ..
        }
    ));
}

#[test]
fn singbox_rejects_snell_v4_jls_obfs() {
    let node = make_snell_node(SnellVersion::V4, Some(SnellObfsMode::Jls));
    let cap = capability_for(ProfileKind::SingBox);
    let err = check_node(&node, &cap).expect_err("sing-box should reject snell v4 jls");
    assert!(matches!(
        err,
        CompatibilityReason::UnsupportedObfsMode {
            protocol: "snell",
            profile: "sing-box",
            mode: "jls",
            ..
        }
    ));
}

#[test]
fn mihomo_accepts_snell_v4_shadowtls_obfs() {
    let node = make_snell_node(SnellVersion::V4, Some(SnellObfsMode::ShadowTls));
    let cap = capability_for(ProfileKind::Mihomo);
    check_node(&node, &cap).expect("mihomo should accept snell v4 shadow-tls obfs");
}
