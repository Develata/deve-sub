//! Round-trip golden test for VLESS Reality: parse → emit → compare.
//!
//! Covers PARSE-001 (VLESS Reality URI golden), PARSE-012 (IPv6 brackets),
//! PARSE-013 (short-id string), PARSE-014 (allowInsecure=0 → Some(false)),
//! PARSE-015 (absent allowInsecure → None). See ADR-0003 and
//! `docs/plan/05-protocol-engine.md` §"VLESS Reality".

#![allow(clippy::expect_used)]

use deve_sub_domain::{Authentication, ProtocolConfig, ProtocolKind, TransportKind};

const RESERVED_UUID: &str = "00000000-0000-4000-8000-000000000001";
const RESERVED_URI: &str = "vless://00000000-0000-4000-8000-000000000001@[2001:db8::1]:443\
     ?security=reality&type=tcp&allowInsecure=0&sni=example.com&fp=chrome\
     &flow=xtls-rprx-vision&sid=01020304&pbk=TEST_PUBLIC_KEY&encryption=none\
     #IPv6-Test";

/// PARSE-001: Parse the reserved VLESS Reality URI and verify field fidelity.
#[test]
fn vless_reality_parse_field_fidelity() {
    let node = deve_sub_protocol::parse_uri(RESERVED_URI).expect("parse");

    assert_eq!(node.protocol, ProtocolKind::Vless);
    assert_eq!(node.display_name, "IPv6-Test");

    let Authentication::Uuid { uuid } = &node.authentication else {
        panic!("expected Uuid authentication");
    };
    assert_eq!(uuid, RESERVED_UUID);

    // PARSE-012: IPv6 host auto-adds brackets in uri_host.
    assert_eq!(node.endpoint.host.uri_host(), "[2001:db8::1]");
    assert_eq!(node.endpoint.port, 443);

    let tls = node.tls.as_ref().expect("tls present");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("example.com"));
    assert_eq!(tls.client_fingerprint.as_deref(), Some("chrome"));

    // PARSE-014: allowInsecure=0 → Some(false).
    assert_eq!(tls.skip_cert_verify, Some(false));

    let transport = node.transport.as_ref().expect("transport present");
    assert_eq!(transport.kind, TransportKind::Tcp);

    let reality = tls.reality.as_ref().expect("reality present");
    assert_eq!(reality.public_key, "TEST_PUBLIC_KEY");
    // PARSE-013: short-id stays string.
    assert_eq!(reality.short_id, "01020304");

    let ProtocolConfig::VlessReality(cfg) = &node.config else {
        panic!("expected VlessReality config");
    };
    assert_eq!(cfg.encryption.as_deref(), Some("none"));
    assert_eq!(cfg.flow.as_deref(), Some("xtls-rprx-vision"));
}

/// PARSE-001: Full round-trip — parse → emit → parse → compare nodes.
#[test]
fn vless_reality_round_trip_semantic() {
    let parsed1 = deve_sub_protocol::parse_uri(RESERVED_URI).expect("parse 1");
    let emitted = deve_sub_emitter::emit_uri(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::parse_uri(&emitted).expect("parse 2");

    // The two parsed nodes should be semantically identical (same protocol,
    // config, endpoint, auth, TLS, transport, etc.).
    assert_eq!(parsed1.protocol, parsed2.protocol);
    assert_eq!(parsed1.config, parsed2.config);
    assert_eq!(parsed1.endpoint, parsed2.endpoint);
    assert_eq!(parsed1.authentication, parsed2.authentication);
    assert_eq!(parsed1.tls, parsed2.tls);
    assert_eq!(parsed1.transport, parsed2.transport);
    assert_eq!(parsed1.display_name, parsed2.display_name);
    assert_eq!(parsed1.udp, parsed2.udp);
}

/// PARSE-001: Emitted URI matches the original (deterministic output).
#[test]
fn vless_reality_round_trip_exact_string() {
    let node = deve_sub_protocol::parse_uri(RESERVED_URI).expect("parse");
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    assert_eq!(emitted, RESERVED_URI);
}

/// PARSE-015: Absent allowInsecure → None.
#[test]
fn absent_allow_insecure_is_none() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&type=tcp&sni=example.com&pbk=TEST_PUBLIC_KEY&sid=01020304\
               #Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let tls = node.tls.as_ref().expect("tls present");
    assert!(tls.skip_cert_verify.is_none());
}

/// PARSE-014: allowInsecure=1 → Some(true).
#[test]
fn allow_insecure_one_is_some_true() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&type=tcp&allowInsecure=1&sni=example.com\
               &pbk=TEST_PUBLIC_KEY&sid=01020304#Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let tls = node.tls.as_ref().expect("tls present");
    assert_eq!(tls.skip_cert_verify, Some(true));
}

/// PARSE-012: IPv4 host round-trip.
#[test]
fn ipv4_host_round_trip() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@192.168.1.1:443\
               ?security=reality&type=tcp&sni=example.com&pbk=TEST_PUBLIC_KEY&sid=01020304\
               #IPv4-Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert_eq!(node.endpoint.host.uri_host(), "192.168.1.1");
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(node.endpoint, reparsed.endpoint);
}

/// Non-Reality VLESS is preserved as Unsupported, not dropped.
#[test]
fn non_reality_vless_is_unsupported() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?type=tcp#NonReality";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert_eq!(node.protocol, ProtocolKind::Vless);
    assert!(matches!(node.config, ProtocolConfig::Unsupported(_)));
}

// ---------------------------------------------------------------------------
// Additional field-coverage round-trip tests
// ---------------------------------------------------------------------------

/// UDP and XUDP flags round-trip through parse → emit → parse.
#[test]
fn udp_xudp_flags_round_trip() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&type=tcp&sni=example.com&pbk=TEST_PUBLIC_KEY&sid=01020304\
               &udp=true&xudp=true#UDP-Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert_eq!(node.udp.supported, Some(true));
    assert_eq!(node.udp.xudp, Some(true));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.udp, node.udp);
}

/// `packetEncoding` field round-trips.
#[test]
fn packet_encoding_round_trip() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&type=tcp&sni=example.com&pbk=TEST_PUBLIC_KEY&sid=01020304\
               &packetEncoding=packet#PE-Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let ProtocolConfig::VlessReality(cfg) = &node.config else {
        panic!("expected VlessReality");
    };
    assert_eq!(cfg.packet_encoding.as_deref(), Some("packet"));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.config, node.config);
}

/// Multi-value ALPN list round-trips.
#[test]
fn alpn_multi_value_round_trip() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&type=tcp&sni=example.com&pbk=TEST_PUBLIC_KEY&sid=01020304\
               &alpn=h2,http/1.1#ALPN-Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let tls = node.tls.as_ref().expect("tls");
    assert_eq!(tls.alpn, vec!["h2", "http/1.1"]);

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.tls, node.tls);
}

/// Empty `alpn=` produces an empty vec, not `vec![""]`.
#[test]
fn alpn_empty_produces_empty_vec() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&type=tcp&sni=example.com&pbk=TEST_PUBLIC_KEY&sid=01020304\
               &alpn=#ALPN-Empty";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let tls = node.tls.as_ref().expect("tls");
    assert!(tls.alpn.is_empty());
}

/// WebSocket transport with path and host round-trips.
#[test]
fn ws_transport_round_trip() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&type=ws&sni=example.com&pbk=TEST_PUBLIC_KEY&sid=01020304\
               &path=/ws&host=ws.example.com#WS-Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let transport = node.transport.as_ref().expect("transport");
    assert_eq!(transport.kind, TransportKind::Ws);
    assert_eq!(transport.path.as_deref(), Some("/ws"));
    assert_eq!(transport.host.as_deref(), Some("ws.example.com"));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.transport, node.transport);
}

/// Display name with space round-trips without double-encoding.
#[test]
fn display_name_with_space_round_trip() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&type=tcp&sni=example.com&pbk=TEST_PUBLIC_KEY&sid=01020304\
               #Hello%20World";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    assert_eq!(node.display_name, "Hello World");

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.display_name, "Hello World");
}

/// Query value with reserved characters (`&`, `=`) round-trips correctly.
#[test]
fn query_value_with_reserved_chars_round_trip() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&type=tcp&sni=example.com&pbk=TEST_PUBLIC_KEY&sid=01020304\
               &spx=%2Fpath%26with%26ampersand#Spx-Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let tls = node.tls.as_ref().expect("tls");
    let reality = tls.reality.as_ref().expect("reality");
    assert_eq!(reality.spider_x.as_deref(), Some("/path&with&ampersand"));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.tls, node.tls);
}

/// gRPC transport round-trips.
#[test]
fn grpc_transport_round_trip() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&type=grpc&sni=example.com&pbk=TEST_PUBLIC_KEY&sid=01020304\
               &path=gun#gRPC-Test";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");
    let transport = node.transport.as_ref().expect("transport");
    assert_eq!(transport.kind, TransportKind::Grpc);
    assert_eq!(transport.path.as_deref(), Some("gun"));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.transport, node.transport);
}

// ---------------------------------------------------------------------------
// Negative tests — malformed inputs must return Err, not panic
// ---------------------------------------------------------------------------

/// Missing UUID returns `MissingField("uuid")`.
#[test]
fn missing_uuid_returns_error() {
    let uri = "vless://@example.com:443?security=reality&pbk=TEST_PUBLIC_KEY&sid=01020304#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("uuid")
    ));
}

/// Missing port returns `MissingField("port")`.
#[test]
fn missing_port_returns_error() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com\
               ?security=reality&pbk=TEST_PUBLIC_KEY&sid=01020304#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("port")
    ));
}

/// Missing `pbk` in Reality returns `MissingField`.
#[test]
fn missing_pbk_returns_error() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&sid=01020304#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::MissingField("pbk (reality public key)")
    ));
}

/// Unknown scheme returns `UnknownScheme`.
#[test]
fn unknown_scheme_returns_error() {
    let uri = "unknown://00000000-0000-4000-8000-000000000001@example.com:443#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(err, deve_sub_protocol::ParseError::UnknownScheme(s) if s == "unknown"));
}

/// Invalid transport type returns `InvalidField`.
#[test]
fn invalid_transport_type_returns_error() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&type=invalid&pbk=TEST_PUBLIC_KEY&sid=01020304#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::InvalidField {
            field: "type (transport)",
            ..
        }
    ));
}

/// Invalid `allowInsecure` value returns `InvalidField`.
#[test]
fn invalid_allow_insecure_returns_error() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&allowInsecure=2&pbk=TEST_PUBLIC_KEY&sid=01020304#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::InvalidField {
            field: "insecure",
            ..
        }
    ));
}

/// Invalid boolean value for `udp` returns `InvalidField`.
#[test]
fn invalid_udp_value_returns_error() {
    let uri = "vless://00000000-0000-4000-8000-000000000001@example.com:443\
               ?security=reality&udp=yes&pbk=TEST_PUBLIC_KEY&sid=01020304#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::InvalidField {
            field: "boolean",
            ..
        }
    ));
}

/// Emitting a VlessReality node without `tls.reality` returns `MissingField`.
#[test]
fn emit_without_reality_returns_error() {
    let mut node = deve_sub_protocol::parse_uri(RESERVED_URI).expect("parse");
    node.tls.as_mut().expect("tls").reality = None;
    let err = deve_sub_emitter::emit_uri(&node).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_emitter::EmitError::MissingField("tls.reality")
    ));
}
