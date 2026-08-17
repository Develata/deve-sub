//! Round-trip golden test for Hysteria2: parse → emit → compare.
//!
//! Covers PARSE-002 (HY2 URI golden).

#![allow(clippy::expect_used)]

use deve_sub_domain::{
    Authentication, CongestionController, Obfuscation, ProtocolConfig, ProtocolKind,
};

/// PARSE-002: Parse the Hysteria2 URI and verify field fidelity.
#[test]
fn hysteria2_parse_field_fidelity() {
    let uri = "hysteria2://TEST_PASSWORD@example.com:443\
               ?sni=example.com&alpn=h3&insecure=0&obfs=salamander&obfs-password=OBFS_PASS\
               &up=100 Mbps&down=200 Mbps&ports=20000-40000&hop_interval=30&fast-open=1&lazy=0\
               #HY2-Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");

    assert_eq!(node.protocol, ProtocolKind::Hysteria2);
    assert_eq!(node.display_name, "HY2-Test");

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password authentication");
    };
    assert_eq!(password, "TEST_PASSWORD");

    assert_eq!(node.endpoint.host.uri_host(), "example.com");
    assert_eq!(node.endpoint.port, 443);

    let tls = node.tls.as_ref().expect("tls present");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("example.com"));
    assert_eq!(tls.skip_cert_verify, Some(false));
    assert_eq!(tls.alpn, vec!["h3"]);

    let obfs = node.obfuscation.as_ref().expect("obfs");
    assert_eq!(obfs.kind, "salamander");
    assert_eq!(obfs.password.as_deref(), Some("OBFS_PASS"));

    let cong = node.congestion.as_ref().expect("congestion");
    assert!(matches!(cong.controller, CongestionController::Bbr));
    assert_eq!(cong.up_bps, Some(100_000_000));
    assert_eq!(cong.down_bps, Some(200_000_000));

    let ProtocolConfig::Hysteria2(cfg) = &node.config else {
        panic!("expected Hysteria2 config");
    };
    assert_eq!(cfg.ports.as_deref(), Some("20000-40000"));
    assert_eq!(cfg.hop_interval, Some(time::Duration::seconds(30)));
    assert_eq!(cfg.fast_open, Some(true));
    assert_eq!(cfg.lazy, Some(false));
}

/// Full round-trip: parse → emit → parse → compare nodes.
#[test]
fn hysteria2_round_trip_semantic() {
    let uri = "hysteria2://TEST_PASSWORD@example.com:443\
               ?sni=example.com&insecure=0&up=100 Mbps&down=200 Mbps&fast-open=1#HY2-RT";
    let parsed1 = deve_sub_protocol::parse_uri(uri).expect("parse 1");
    let emitted = deve_sub_emitter::emit_uri(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::parse_uri(&emitted).expect("parse 2");

    assert_eq!(parsed1.protocol, parsed2.protocol);
    assert_eq!(parsed1.config, parsed2.config);
    assert_eq!(parsed1.endpoint, parsed2.endpoint);
    assert_eq!(parsed1.authentication, parsed2.authentication);
    assert_eq!(parsed1.tls, parsed2.tls);
    assert_eq!(parsed1.obfuscation, parsed2.obfuscation);
    assert_eq!(parsed1.congestion, parsed2.congestion);
    assert_eq!(parsed1.display_name, parsed2.display_name);
}

/// hy2:// scheme alias works.
#[test]
fn hysteria2_hy2_alias() {
    let uri = "hy2://TEST_PASSWORD@example.com:443?sni=example.com#HY2-Alias";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert_eq!(node.protocol, ProtocolKind::Hysteria2);
}

/// Obfuscation round-trips.
#[test]
fn hysteria2_obfs_round_trip() {
    let uri = "hysteria2://TEST_PASSWORD@example.com:443?obfs=salamander&obfs-password=OBFS#Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let obfs = node.obfuscation.as_ref().expect("obfs");
    assert_eq!(obfs.kind, "salamander");
    assert_eq!(obfs.password.as_deref(), Some("OBFS"));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.obfuscation, node.obfuscation);
}

/// Bandwidth round-trips.
#[test]
fn hysteria2_bandwidth_round_trip() {
    let uri = "hysteria2://TEST_PASSWORD@example.com:443?up=50 Mbps&down=100 Mbps#BW";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let cong = node.congestion.as_ref().expect("congestion");
    assert_eq!(cong.up_bps, Some(50_000_000));
    assert_eq!(cong.down_bps, Some(100_000_000));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.congestion, node.congestion);
}

/// Port hopping range round-trips.
#[test]
fn hysteria2_ports_round_trip() {
    let uri = "hysteria2://TEST_PASSWORD@example.com:443?ports=20000-40000#Ports";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let ProtocolConfig::Hysteria2(cfg) = &node.config else {
        panic!("expected Hysteria2");
    };
    assert_eq!(cfg.ports.as_deref(), Some("20000-40000"));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.config, node.config);
}

/// IPv6 host round-trip.
#[test]
fn hysteria2_ipv6_round_trip() {
    let uri = "hysteria2://TEST_PASSWORD@[2001:db8::1]:443?sni=example.com#IPv6";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert_eq!(node.endpoint.host.uri_host(), "[2001:db8::1]");
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(node.endpoint, reparsed.endpoint);
}

/// Missing password returns error.
#[test]
fn hysteria2_missing_password_returns_error() {
    let uri = "hysteria2://@example.com:443#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("password")
    ));
}

/// Missing port returns error.
#[test]
fn hysteria2_missing_port_returns_error() {
    let uri = "hysteria2://TEST_PASSWORD@example.com#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("port")
    ));
}

/// hop_interval round-trips.
#[test]
fn hysteria2_hop_interval_round_trip() {
    let uri = "hysteria2://TEST_PASSWORD@example.com:443?hop_interval=60#Hop";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let ProtocolConfig::Hysteria2(cfg) = &node.config else {
        panic!("expected Hysteria2");
    };
    assert_eq!(cfg.hop_interval, Some(time::Duration::seconds(60)));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.config, node.config);
}

/// TLS is always present for Hysteria2 (QUIC-based protocol).
#[test]
fn hysteria2_tls_always_present() {
    let uri = "hysteria2://TEST_PASSWORD@example.com:443#NoTLS";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let tls = node
        .tls
        .as_ref()
        .expect("tls must be present for Hysteria2");
    assert!(tls.enabled);
}

/// Obfuscation struct is correctly typed.
#[test]
fn hysteria2_obfuscation_type() {
    let uri = "hysteria2://TEST_PASSWORD@example.com:443?obfs=salamander&obfs-password=x#Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert!(node.obfuscation.is_some());
    let obfs: &Obfuscation = node.obfuscation.as_ref().expect("obfs");
    assert_eq!(obfs.kind, "salamander");
}

/// W-U: congestion-controller query param must be parsed (previously
/// hardcoded to Bbr, silently dropping non-Bbr controllers on round-trip).
#[test]
fn hysteria2_congestion_controller_round_trip() {
    let uri = "hysteria2://TEST_PASSWORD@example.com:443\
               ?up=50 Mbps&down=100 Mbps&congestion-controller=cubic#Cong";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let cong = node.congestion.as_ref().expect("congestion");
    assert!(matches!(cong.controller, CongestionController::Cubic));
    assert_eq!(cong.up_bps, Some(50_000_000));
    assert_eq!(cong.down_bps, Some(100_000_000));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    assert!(
        emitted.contains("congestion-controller=cubic"),
        "emitted URI must contain congestion-controller=cubic, got: {emitted}"
    );
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.congestion, node.congestion);
}

/// W-U: congestion-controller is preserved even without up/down bandwidth.
#[test]
fn hysteria2_congestion_controller_without_bandwidth() {
    let uri = "hysteria2://TEST_PASSWORD@example.com:443?congestion-controller=new_reno#NoBW";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let cong = node
        .congestion
        .as_ref()
        .expect("congestion must be present");
    assert!(
        matches!(cong.controller, CongestionController::NewReno),
        "controller must be NewReno"
    );
    assert_eq!(cong.up_bps, None);
    assert_eq!(cong.down_bps, None);

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.congestion, node.congestion);
}
