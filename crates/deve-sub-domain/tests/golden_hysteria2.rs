//! Golden scaffold for Hysteria2: construct the canonical model with reserved
//! test identifiers, assert field fidelity (obfuscation, congestion, port
//! hopping), and round-trip through serde. Full URI parse → emit round-trip is
//! M3 work (acceptance `PARSE-002`). See ADR-0003 and
//! `docs/plan/05-protocol-engine.md` §"Hysteria2".

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use deve_sub_domain::{
    Authentication, CertificatePin, CongestionConfig, CongestionController, Endpoint, Host,
    Hysteria2Config, Node, NodeSource, Obfuscation, ProtocolConfig, ProtocolKind, RegionAssignment,
    RegionMethod, TlsConfig, UdpCapability,
};
use deve_sub_kernel::{NodeId, Timestamp};

fn build_hysteria2_node() -> Node {
    Node {
        id: NodeId::new(),
        display_name: "HY2-Reserved-Test".to_owned(),
        protocol: ProtocolKind::Hysteria2,
        config: ProtocolConfig::Hysteria2(Hysteria2Config {
            ports: Some("20000-40000".to_owned()),
            hop_interval: Some(time::Duration::seconds(30)),
            fast_open: Some(true),
            lazy: None,
        }),
        endpoint: Endpoint {
            host: Host::Ipv6("2001:db8::1".parse().expect("valid reserved IPv6")),
            port: 443,
        },
        authentication: Authentication::Password {
            password: "TEST_PASSWORD".to_owned(),
        },
        transport: None,
        tls: Some(TlsConfig {
            enabled: true,
            server_name: Some("hy2.test.example".to_owned()),
            skip_cert_verify: None,
            alpn: vec!["h3".to_owned()],
            client_fingerprint: None,
            certificate_pins: vec![CertificatePin::new("pinSHA256:TEST_PIN".to_owned())],
            reality: None,
        }),
        udp: UdpCapability {
            supported: Some(true),
            xudp: None,
        },
        multiplex: None,
        obfuscation: Some(Obfuscation {
            kind: "salamander".to_owned(),
            password: Some("TEST_OBFS_PASSWORD".to_owned()),
        }),
        congestion: Some(CongestionConfig {
            controller: CongestionController::Bbr,
            up_bps: Some(100_000_000),
            down_bps: Some(100_000_000),
        }),
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
fn hysteria2_fields_match_reserved_fixture() {
    let node = build_hysteria2_node();

    assert_eq!(node.protocol, ProtocolKind::Hysteria2);
    assert_eq!(node.endpoint.host.uri_host(), "[2001:db8::1]");
    assert_eq!(node.endpoint.port, 443);

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password authentication");
    };
    assert_eq!(password, "TEST_PASSWORD");

    let tls = node.tls.as_ref().expect("tls present");
    assert_eq!(tls.skip_cert_verify, None);
    assert_eq!(tls.alpn, vec!["h3".to_owned()]);

    let obfs = node.obfuscation.as_ref().expect("obfuscation present");
    assert_eq!(obfs.kind, "salamander");
    assert_eq!(obfs.password.as_deref(), Some("TEST_OBFS_PASSWORD"));

    let cong = node.congestion.as_ref().expect("congestion present");
    assert_eq!(cong.controller, CongestionController::Bbr);
    assert_eq!(cong.up_bps, Some(100_000_000));
    assert_eq!(cong.down_bps, Some(100_000_000));

    let ProtocolConfig::Hysteria2(cfg) = &node.config else {
        panic!("expected Hysteria2 config");
    };
    assert_eq!(cfg.ports.as_deref(), Some("20000-40000"));
    assert_eq!(cfg.hop_interval, Some(time::Duration::seconds(30)));
    assert_eq!(cfg.fast_open, Some(true));
    assert_eq!(cfg.lazy, None);
}

#[test]
fn hysteria2_serde_roundtrip() {
    let node = build_hysteria2_node();
    let json = serde_json::to_string(&node).expect("serialize");
    let recovered: Node = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(node, recovered);
}

#[test]
fn hysteria2_absent_allow_insecure_is_none() {
    let node = build_hysteria2_node();
    let tls = node.tls.as_ref().expect("tls present");
    assert!(tls.skip_cert_verify.is_none());
    let json = serde_json::to_string(&node).expect("serialize");
    assert!(json.contains("\"skip_cert_verify\":null"));
}
