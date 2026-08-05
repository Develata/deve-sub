//! Round-trip golden test for TUIC v5: parse → emit → compare.
//!
//! Covers PARSE-003 (TUIC v5 URI golden).

#![allow(clippy::expect_used)]

use deve_sub_domain::{
    Authentication, CongestionController, ProtocolConfig, ProtocolKind, UdpRelayMode,
};

const RESERVED_UUID: &str = "00000000-0000-4000-8000-000000000001";

/// PARSE-003: Parse the TUIC v5 URI and verify field fidelity.
#[test]
fn tuic_v5_parse_field_fidelity() {
    let uri = "tuic://00000000-0000-4000-8000-000000000001:TEST_PASSWORD@example.com:443\
               ?sni=example.com&alpn=h3&skip-cert-verify=0&congestion-controller=bbr\
               &udp-relay-mode=native&zero-rtt-handshake=1&heartbeat=10000&disable-sni=0\
               #TUIC-Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");

    assert_eq!(node.protocol, ProtocolKind::TuicV5);
    assert_eq!(node.display_name, "TUIC-Test");

    let Authentication::UuidPassword { uuid, password } = &node.authentication else {
        panic!("expected UuidPassword authentication");
    };
    assert_eq!(uuid, RESERVED_UUID);
    assert_eq!(password, "TEST_PASSWORD");

    assert_eq!(node.endpoint.host.uri_host(), "example.com");
    assert_eq!(node.endpoint.port, 443);

    let tls = node.tls.as_ref().expect("tls present");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("example.com"));
    assert_eq!(tls.skip_cert_verify, Some(false));
    assert_eq!(tls.alpn, vec!["h3"]);

    let cong = node.congestion.as_ref().expect("congestion");
    assert!(matches!(cong.controller, CongestionController::Bbr));

    let ProtocolConfig::TuicV5(cfg) = &node.config else {
        panic!("expected TuicV5 config");
    };
    assert_eq!(cfg.udp_relay_mode, Some(UdpRelayMode::Native));
    assert_eq!(cfg.zero_rtt_handshake, Some(true));
    assert_eq!(cfg.heartbeat, Some(time::Duration::milliseconds(10000)));
    assert_eq!(cfg.disable_sni, Some(false));
}

/// Full round-trip: parse → emit → parse → compare nodes.
#[test]
fn tuic_v5_round_trip_semantic() {
    let uri = "tuic://00000000-0000-4000-8000-000000000001:TEST_PASSWORD@example.com:443\
               ?sni=example.com&congestion-controller=cubic&udp-relay-mode=quic\
               &zero-rtt-handshake=1&heartbeat=5000#TUIC-RT";
    let parsed1 = deve_sub_protocol::parse_uri(uri).expect("parse 1");
    let emitted = deve_sub_emitter::emit_uri(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::parse_uri(&emitted).expect("parse 2");

    assert_eq!(parsed1.protocol, parsed2.protocol);
    assert_eq!(parsed1.config, parsed2.config);
    assert_eq!(parsed1.endpoint, parsed2.endpoint);
    assert_eq!(parsed1.authentication, parsed2.authentication);
    assert_eq!(parsed1.tls, parsed2.tls);
    assert_eq!(parsed1.congestion, parsed2.congestion);
    assert_eq!(parsed1.display_name, parsed2.display_name);
}

/// Heartbeat round-trips.
#[test]
fn tuic_v5_heartbeat_round_trip() {
    let uri = "tuic://00000000-0000-4000-8000-000000000001:TEST_PASSWORD@example.com:443\
               ?heartbeat=3000#HB";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let ProtocolConfig::TuicV5(cfg) = &node.config else {
        panic!("expected TuicV5");
    };
    assert_eq!(cfg.heartbeat, Some(time::Duration::milliseconds(3000)));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.config, node.config);
}

/// udp-relay-mode round-trips.
#[test]
fn tuic_v5_udp_relay_mode_round_trip() {
    let uri = "tuic://00000000-0000-4000-8000-000000000001:TEST_PASSWORD@example.com:443\
               ?udp-relay-mode=quic#UDP";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let ProtocolConfig::TuicV5(cfg) = &node.config else {
        panic!("expected TuicV5");
    };
    assert_eq!(cfg.udp_relay_mode, Some(UdpRelayMode::Quic));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.config, node.config);
}

/// congestion-controller round-trips.
#[test]
fn tuic_v5_congestion_controller_round_trip() {
    let uri = "tuic://00000000-0000-4000-8000-000000000001:TEST_PASSWORD@example.com:443\
               ?congestion-controller=cubic#CC";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let cong = node.congestion.as_ref().expect("congestion");
    assert!(matches!(cong.controller, CongestionController::Cubic));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.congestion, node.congestion);
}

/// IPv6 host round-trip.
#[test]
fn tuic_v5_ipv6_round_trip() {
    let uri = "tuic://00000000-0000-4000-8000-000000000001:TEST_PASSWORD@[2001:db8::1]:443\
               ?sni=example.com#IPv6";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert_eq!(node.endpoint.host.uri_host(), "[2001:db8::1]");
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(node.endpoint, reparsed.endpoint);
}

/// Missing UUID returns error.
#[test]
fn tuic_v5_missing_uuid_returns_error() {
    let uri = "tuic://:TEST_PASSWORD@example.com:443#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("uuid")
    ));
}

/// Missing password returns error.
#[test]
fn tuic_v5_missing_password_returns_error() {
    let uri = "tuic://00000000-0000-4000-8000-000000000001@example.com:443#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("password")
    ));
}

/// Invalid udp-relay-mode returns error.
#[test]
fn tuic_v5_invalid_udp_relay_mode_returns_error() {
    let uri = "tuic://00000000-0000-4000-8000-000000000001:TEST_PASSWORD@example.com:443\
               ?udp-relay-mode=invalid#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::InvalidField {
            field: "udp-relay-mode",
            ..
        }
    ));
}
