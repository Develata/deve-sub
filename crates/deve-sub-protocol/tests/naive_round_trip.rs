//! Round-trip golden test for NaiveProxy: parse → emit → compare.
//!
//! Covers PARSE-004 (Naive URI golden — must not downgrade to plain HTTP).

#![allow(clippy::expect_used)]

use deve_sub_domain::{Authentication, NaiveProxyConfig, ProtocolConfig, ProtocolKind};

/// PARSE-004: Parse a naive+https:// URI and verify field fidelity.
#[test]
fn naive_parse_field_fidelity() {
    let uri = "naive+https://user:pass@example.com:443\
               ?sni=example.com&alpn=h2&skip-cert-verify=0&quic=1&http2=1&http3=0#Naive-Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");

    assert_eq!(node.protocol, ProtocolKind::NaiveProxy);
    assert_eq!(node.display_name, "Naive-Test");

    let Authentication::UserPassword { username, password } = &node.authentication else {
        panic!("expected UserPassword authentication");
    };
    assert_eq!(username, "user");
    assert_eq!(password, "pass");

    assert_eq!(node.endpoint.host.uri_host(), "example.com");
    assert_eq!(node.endpoint.port, 443);

    // WHY: naive+https must have TLS — must not downgrade to plain HTTP.
    let tls = node
        .tls
        .as_ref()
        .expect("tls must be present for naive+https");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("example.com"));
    assert_eq!(tls.skip_cert_verify, Some(false));
    assert_eq!(tls.alpn, vec!["h2"]);

    let ProtocolConfig::NaiveProxy(NaiveProxyConfig { quic, http2, http3 }) = &node.config else {
        panic!("expected NaiveProxy config");
    };
    assert_eq!(*quic, Some(true));
    assert_eq!(*http2, Some(true));
    assert_eq!(*http3, Some(false));
}

/// Full round-trip: parse → emit → parse → compare nodes.
#[test]
fn naive_round_trip_semantic() {
    let uri = "naive+https://user:pass@example.com:443?sni=example.com&quic=1#Naive-RT";
    let parsed1 = deve_sub_protocol::parse_uri(uri).expect("parse 1");
    let emitted = deve_sub_emitter::emit_uri(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::parse_uri(&emitted).expect("parse 2");

    assert_eq!(parsed1.protocol, parsed2.protocol);
    assert_eq!(parsed1.config, parsed2.config);
    assert_eq!(parsed1.endpoint, parsed2.endpoint);
    assert_eq!(parsed1.authentication, parsed2.authentication);
    assert_eq!(parsed1.tls, parsed2.tls);
    assert_eq!(parsed1.display_name, parsed2.display_name);
}

/// PARSE-004: naive+https must emit as naive+https (no downgrade to http).
#[test]
fn naive_https_no_downgrade() {
    let uri = "naive+https://user:pass@example.com:443?sni=example.com#NoDowngrade";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert!(node.tls.is_some());
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    assert!(
        emitted.starts_with("naive+https://"),
        "emitted URI must start with naive+https://, got: {emitted}"
    );
}

/// naive+http produces no TLS.
#[test]
fn naive_http_no_tls() {
    let uri = "naive+http://user:pass@example.com:8080#NaiveHTTP";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert!(node.tls.is_none());

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    assert!(emitted.starts_with("naive+http://"));
}

/// IPv6 host round-trip.
#[test]
fn naive_ipv6_round_trip() {
    let uri = "naive+https://user:pass@[2001:db8::1]:443?sni=example.com#IPv6";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert_eq!(node.endpoint.host.uri_host(), "[2001:db8::1]");
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(node.endpoint, reparsed.endpoint);
}

/// Missing username returns error.
#[test]
fn naive_missing_username_returns_error() {
    let uri = "naive+https://:pass@example.com:443#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("username")
    ));
}

/// Missing password returns error.
#[test]
fn naive_missing_password_returns_error() {
    let uri = "naive+https://user@example.com:443#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("password")
    ));
}

/// quic/http2/http3 flags round-trip.
#[test]
fn naive_flags_round_trip() {
    let uri = "naive+https://user:pass@example.com:443?quic=0&http2=1&http3=1#Flags";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let ProtocolConfig::NaiveProxy(cfg) = &node.config else {
        panic!("expected NaiveProxy");
    };
    assert_eq!(cfg.quic, Some(false));
    assert_eq!(cfg.http2, Some(true));
    assert_eq!(cfg.http3, Some(true));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.config, node.config);
}
