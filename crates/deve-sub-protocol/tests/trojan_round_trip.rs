//! Round-trip golden test for Trojan: parse → emit → compare.
//!
//! Trojan round-trip is part of PARSE-017 property coverage.

#![allow(clippy::expect_used)]

use deve_sub_domain::{Authentication, ProtocolConfig, ProtocolKind, TransportKind};

const RESERVED_PASSWORD: &str = "TEST_PASSWORD";

/// Field fidelity: parse a Trojan URI and verify all fields.
#[test]
fn trojan_parse_field_fidelity() {
    let uri = "trojan://TEST_PASSWORD@example.com:443\
               ?sni=example.com&alpn=h2,http/1.1&skip-cert-verify=0&type=ws\
               &path=/ws&host=ws.example.com&packetEncoding=packet#Trojan-Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");

    assert_eq!(node.protocol, ProtocolKind::Trojan);
    assert_eq!(node.display_name, "Trojan-Test");

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password authentication");
    };
    assert_eq!(password, RESERVED_PASSWORD);

    assert_eq!(node.endpoint.host.uri_host(), "example.com");
    assert_eq!(node.endpoint.port, 443);

    let tls = node.tls.as_ref().expect("tls present");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("example.com"));
    assert_eq!(tls.skip_cert_verify, Some(false));
    assert_eq!(tls.alpn, vec!["h2", "http/1.1"]);

    let transport = node.transport.as_ref().expect("transport");
    assert_eq!(transport.kind, TransportKind::Ws);
    assert_eq!(transport.path.as_deref(), Some("/ws"));
    assert_eq!(transport.host.as_deref(), Some("ws.example.com"));

    let ProtocolConfig::Trojan(cfg) = &node.config else {
        panic!("expected Trojan config");
    };
    assert_eq!(cfg.packet_encoding.as_deref(), Some("packet"));
}

/// Full round-trip: parse → emit → parse → compare nodes.
#[test]
fn trojan_round_trip_semantic() {
    let uri = "trojan://TEST_PASSWORD@example.com:443\
               ?sni=example.com&skip-cert-verify=0&type=tcp#Trojan-RT";
    let parsed1 = deve_sub_protocol::parse_uri(uri).expect("parse 1");
    let emitted = deve_sub_emitter::emit_uri(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::parse_uri(&emitted).expect("parse 2");

    assert_eq!(parsed1.protocol, parsed2.protocol);
    assert_eq!(parsed1.config, parsed2.config);
    assert_eq!(parsed1.endpoint, parsed2.endpoint);
    assert_eq!(parsed1.authentication, parsed2.authentication);
    assert_eq!(parsed1.tls, parsed2.tls);
    assert_eq!(parsed1.transport, parsed2.transport);
    assert_eq!(parsed1.display_name, parsed2.display_name);
}

/// Absent skip-cert-verify → None.
#[test]
fn trojan_absent_skip_cert_verify_is_none() {
    let uri = "trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let tls = node.tls.as_ref().expect("tls");
    assert!(tls.skip_cert_verify.is_none());
}

/// skip-cert-verify=1 → Some(true).
#[test]
fn trojan_skip_cert_verify_one_is_some_true() {
    let uri = "trojan://TEST_PASSWORD@example.com:443?skip-cert-verify=1&type=tcp#Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let tls = node.tls.as_ref().expect("tls");
    assert_eq!(tls.skip_cert_verify, Some(true));
}

/// IPv6 host round-trip.
#[test]
fn trojan_ipv6_round_trip() {
    let uri = "trojan://TEST_PASSWORD@[2001:db8::1]:443?sni=example.com&type=tcp#IPv6";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert_eq!(node.endpoint.host.uri_host(), "[2001:db8::1]");
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(node.endpoint, reparsed.endpoint);
}

/// Missing password returns error.
#[test]
fn trojan_missing_password_returns_error() {
    let uri = "trojan://@example.com:443?type=tcp#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("password")
    ));
}

/// Missing port returns error.
#[test]
fn trojan_missing_port_returns_error() {
    let uri = "trojan://TEST_PASSWORD@example.com?type=tcp#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("port")
    ));
}

/// Display name with space round-trips without double-encoding.
#[test]
fn trojan_display_name_with_space_round_trip() {
    let uri = "trojan://TEST_PASSWORD@example.com:443?type=tcp#Hello%20World";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert_eq!(node.display_name, "Hello World");
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.display_name, "Hello World");
}

/// ALPN multi-value round-trips.
#[test]
fn trojan_alpn_round_trip() {
    let uri = "trojan://TEST_PASSWORD@example.com:443?alpn=h2,http/1.1&type=tcp#Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let tls = node.tls.as_ref().expect("tls");
    assert_eq!(tls.alpn, vec!["h2", "http/1.1"]);
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.tls, node.tls);
}

/// gRPC transport round-trips.
#[test]
fn trojan_grpc_round_trip() {
    let uri = "trojan://TEST_PASSWORD@example.com:443?type=grpc&path=gun#gRPC";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let transport = node.transport.as_ref().expect("transport");
    assert_eq!(transport.kind, TransportKind::Grpc);
    assert_eq!(transport.path.as_deref(), Some("gun"));
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.transport, node.transport);
}

/// W-B: password with URI-reserved characters (`@`, `:`, `/`, `?`, `#`, `%`)
/// must round-trip without corrupting the URI structure. Emitters percent-
/// encode userinfo (RFC 3986 §3.2.1); parsers percent-decode it back.
#[test]
fn trojan_reserved_password_round_trip() {
    // WHY: all reserved chars are percent-encoded in the URI so `url::Url`
    // does not split on a literal `:` (trojan uses `password@host`, not
    // `user:password@host`).
    let uri = "trojan://p%40ss%3Aword%2Fq%3Fr%23e@example.com:443?type=tcp#Reserved";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password authentication");
    };
    assert_eq!(password, "p@ss:word/q?r#e");
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    assert!(
        !emitted.contains("p@ss:word/q?r#e@"),
        "emitted URI must percent-encode reserved chars: {emitted}"
    );
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    let Authentication::Password { password: pwd2 } = &reparsed.authentication else {
        panic!("expected Password authentication on reparse");
    };
    assert_eq!(pwd2, "p@ss:word/q?r#e");
}
