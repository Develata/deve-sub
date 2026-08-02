//! Golden scaffold for VLESS Reality: construct the canonical model from the
//! reserved test URI, assert field fidelity (especially the three-state
//! `allowInsecure=0` and string-typed `short_id`), and round-trip through
//! serde. Full URI parse → emit round-trip is M3 work (acceptance
//! `PARSE-001`). See ADR-0003 and `docs/plan/05-protocol-engine.md` §6.1.

use std::collections::BTreeMap;

use deve_sub_domain::{
    Authentication, Endpoint, Host, Node, NodeId, NodeSource, ProtocolConfig, ProtocolKind,
    RealityConfig, RegionAssignment, RegionMethod, TlsConfig, Transport, TransportKind,
    UdpCapability, VlessRealityConfig,
};
use deve_sub_kernel::Timestamp;

/// Reserved test URI from `docs/plan/05-protocol-engine.md` §"Node export":
/// `vless://00000000-0000-4000-8000-000000000001@[2001:db8::1]:443
///   ?security=reality&type=tcp&allowInsecure=0&sni=example.com&fp=chrome
///   &flow=xtls-rprx-vision&sid=01020304&pbk=TEST_PUBLIC_KEY&encryption=none
///   #IPv6-Test`
const RESERVED_UUID: &str = "00000000-0000-4000-8000-000000000001";

fn build_vless_reality_node() -> Node {
    Node {
        id: NodeId::new(),
        display_name: "IPv6-Test".to_owned(),
        protocol: ProtocolKind::Vless,
        config: ProtocolConfig::VlessReality(VlessRealityConfig {
            encryption: Some("none".to_owned()),
            flow: Some("xtls-rprx-vision".to_owned()),
            packet_encoding: None,
        }),
        endpoint: Endpoint {
            host: Host::Ipv6("2001:db8::1".parse().expect("valid reserved IPv6")),
            port: 443,
        },
        authentication: Authentication::Uuid {
            uuid: RESERVED_UUID.to_owned(),
        },
        transport: Some(Transport {
            kind: TransportKind::Tcp,
            path: None,
            host: None,
        }),
        tls: Some(TlsConfig {
            enabled: true,
            server_name: Some("example.com".to_owned()),
            skip_cert_verify: Some(false),
            alpn: vec![],
            client_fingerprint: Some("chrome".to_owned()),
            certificate_pins: vec![],
            reality: Some(RealityConfig {
                public_key: "TEST_PUBLIC_KEY".to_owned(),
                short_id: "01020304".to_owned(),
                spider_x: None,
            }),
        }),
        udp: UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "reserved-test-source".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::now(),
        },
        tags: vec![],
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: None,
        },
        extras: BTreeMap::new(),
    }
}

#[test]
fn vless_reality_fields_match_reserved_uri() {
    let node = build_vless_reality_node();

    assert_eq!(node.protocol, ProtocolKind::Vless);
    assert_eq!(node.display_name, "IPv6-Test");
    assert_eq!(node.endpoint.port, 443);
    assert_eq!(node.endpoint.host.uri_host(), "[2001:db8::1]");

    let Authentication::Uuid { uuid } = &node.authentication else {
        panic!("expected Uuid authentication");
    };
    assert_eq!(uuid, RESERVED_UUID);

    let tls = node.tls.as_ref().expect("tls present");
    assert_eq!(tls.server_name.as_deref(), Some("example.com"));
    assert_eq!(tls.client_fingerprint.as_deref(), Some("chrome"));
    assert_eq!(tls.skip_cert_verify, Some(false));

    let reality = tls.reality.as_ref().expect("reality present");
    assert_eq!(reality.public_key, "TEST_PUBLIC_KEY");
    assert_eq!(reality.short_id, "01020304");

    let ProtocolConfig::VlessReality(cfg) = &node.config else {
        panic!("expected VlessReality config");
    };
    assert_eq!(cfg.encryption.as_deref(), Some("none"));
    assert_eq!(cfg.flow.as_deref(), Some("xtls-rprx-vision"));
}

#[test]
fn vless_reality_serde_roundtrip() {
    let node = build_vless_reality_node();
    let json = serde_json::to_string(&node).expect("serialize");
    let recovered: Node = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(node, recovered);
}

#[test]
fn vless_reality_short_id_stays_string_in_json() {
    let node = build_vless_reality_node();
    let json = serde_json::to_string(&node).expect("serialize");
    assert!(json.contains("\"short_id\":\"01020304\""));
    assert!(!json.contains("\"short_id\":01020304"));
}

#[test]
fn vless_reality_allow_insecure_zero_is_some_false() {
    let node = build_vless_reality_node();
    let tls = node.tls.as_ref().expect("tls present");
    assert_eq!(tls.skip_cert_verify, Some(false));
    let json = serde_json::to_string(&node).expect("serialize");
    assert!(json.contains("\"skip_cert_verify\":false"));
}
