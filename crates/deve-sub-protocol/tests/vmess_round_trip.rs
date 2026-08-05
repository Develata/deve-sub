//! Round-trip golden test for VMess: parse → emit → compare.
//!
//! VMess uses `vmess://BASE64(JSON)` format, fundamentally different from
//! other protocols. Part of PARSE-017 property coverage.

#![allow(clippy::expect_used)]

use base64::Engine;
use deve_sub_domain::{Authentication, ProtocolConfig, ProtocolKind, TransportKind};

const RESERVED_UUID: &str = "00000000-0000-4000-8000-000000000001";

/// Build a VMess URI from a JSON body.
fn vmess_uri(json: &serde_json::Value) -> String {
    let json_str = serde_json::to_string(json).expect("serialize");
    let encoded = base64::engine::general_purpose::STANDARD.encode(json_str.as_bytes());
    format!("vmess://{encoded}")
}

/// Field fidelity: parse a VMess URI and verify all fields.
#[test]
fn vmess_parse_field_fidelity() {
    let json = serde_json::json!({
        "v": "2",
        "ps": "VMess-Test",
        "add": "example.com",
        "port": "443",
        "id": RESERVED_UUID,
        "aid": "0",
        "scy": "auto",
        "net": "ws",
        "type": "none",
        "host": "ws.example.com",
        "path": "/ws",
        "tls": "tls",
        "sni": "example.com",
        "alpn": "h2,http/1.1",
        "packetEncoding": "packet"
    });
    let uri = vmess_uri(&json);
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");

    assert_eq!(node.protocol, ProtocolKind::VMess);
    assert_eq!(node.display_name, "VMess-Test");

    let Authentication::Uuid { uuid } = &node.authentication else {
        panic!("expected Uuid authentication");
    };
    assert_eq!(uuid, RESERVED_UUID);

    assert_eq!(node.endpoint.host.uri_host(), "example.com");
    assert_eq!(node.endpoint.port, 443);

    let tls = node.tls.as_ref().expect("tls");
    assert!(tls.enabled);
    assert_eq!(tls.server_name.as_deref(), Some("example.com"));
    assert_eq!(tls.alpn, vec!["h2", "http/1.1"]);

    let transport = node.transport.as_ref().expect("transport");
    assert_eq!(transport.kind, TransportKind::Ws);
    assert_eq!(transport.path.as_deref(), Some("/ws"));
    assert_eq!(transport.host.as_deref(), Some("ws.example.com"));

    let ProtocolConfig::VMess(cfg) = &node.config else {
        panic!("expected VMess config");
    };
    assert_eq!(cfg.alter_id, Some(0));
    assert_eq!(cfg.security.as_deref(), Some("auto"));
    assert_eq!(cfg.packet_encoding.as_deref(), Some("packet"));
}

/// Full round-trip: parse → emit → parse → compare nodes.
#[test]
fn vmess_round_trip_semantic() {
    let json = serde_json::json!({
        "v": "2",
        "ps": "VMess-RT",
        "add": "example.com",
        "port": "443",
        "id": RESERVED_UUID,
        "aid": "0",
        "scy": "aes-128-gcm",
        "net": "tcp",
        "type": "none",
        "host": "",
        "path": "",
        "tls": "tls",
        "sni": "example.com",
        "alpn": ""
    });
    let uri = vmess_uri(&json);
    let parsed1 = deve_sub_protocol::parse_uri(&uri).expect("parse 1");
    let emitted = deve_sub_emitter::emit_uri(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::parse_uri(&emitted).expect("parse 2");

    assert_eq!(parsed1.protocol, parsed2.protocol);
    assert_eq!(parsed1.config, parsed2.config);
    assert_eq!(parsed1.endpoint, parsed2.endpoint);
    assert_eq!(parsed1.authentication, parsed2.authentication);
    assert_eq!(parsed1.tls, parsed2.tls);
    assert_eq!(parsed1.transport, parsed2.transport);
    assert_eq!(parsed1.display_name, parsed2.display_name);
    assert_eq!(parsed1.extras, parsed2.extras);
}

/// Numeric port is accepted (some implementations use numbers).
#[test]
fn vmess_numeric_port() {
    let json = serde_json::json!({
        "add": "example.com",
        "port": 443,
        "id": RESERVED_UUID,
        "aid": 0,
        "net": "tcp",
        "tls": "tls",
    });
    let uri = vmess_uri(&json);
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    assert_eq!(node.endpoint.port, 443);
    let ProtocolConfig::VMess(cfg) = &node.config else {
        panic!("expected VMess");
    };
    assert_eq!(cfg.alter_id, Some(0));
}

/// No TLS (tls="" or absent).
#[test]
fn vmess_no_tls() {
    let json = serde_json::json!({
        "add": "example.com",
        "port": "80",
        "id": RESERVED_UUID,
        "aid": "0",
        "net": "tcp",
        "tls": "",
    });
    let uri = vmess_uri(&json);
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    assert!(node.tls.is_none());
}

/// gRPC transport round-trips.
#[test]
fn vmess_grpc_round_trip() {
    let json = serde_json::json!({
        "add": "example.com",
        "port": "443",
        "id": RESERVED_UUID,
        "aid": "0",
        "net": "grpc",
        "path": "gun",
        "tls": "tls",
        "sni": "example.com",
    });
    let uri = vmess_uri(&json);
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    let transport = node.transport.as_ref().expect("transport");
    assert_eq!(transport.kind, TransportKind::Grpc);
    assert_eq!(transport.path.as_deref(), Some("gun"));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.transport, node.transport);
}

/// KCP header type (`type: "wire"`) round-trips through `extras`.
#[test]
fn vmess_kcp_header_type_round_trip() {
    let json = serde_json::json!({
        "add": "example.com",
        "port": "443",
        "id": RESERVED_UUID,
        "aid": "0",
        "net": "kcp",
        "type": "wire",
        "tls": "tls",
        "sni": "example.com",
    });
    let uri = vmess_uri(&json);
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");

    // The header type is stored in extras.
    assert_eq!(
        node.extras
            .get("vmess_header_type")
            .and_then(|v| v.as_str()),
        Some("wire")
    );

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.extras, node.extras);
}

/// IPv6 host round-trip (add field is plain IPv6 without brackets).
#[test]
fn vmess_ipv6_round_trip() {
    let json = serde_json::json!({
        "add": "2001:db8::1",
        "port": "443",
        "id": RESERVED_UUID,
        "aid": "0",
        "net": "tcp",
        "tls": "tls",
    });
    let uri = vmess_uri(&json);
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    // The VMess emitter outputs the host without brackets in the JSON `add`
    // field; the URI host bracketing is irrelevant for VMess since it's JSON.
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(node.endpoint, reparsed.endpoint);
}

/// Invalid Base64 returns error.
#[test]
fn vmess_invalid_base64_returns_error() {
    let uri = "vmess://!!!invalid-base64!!!";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::InvalidBase64(_)
    ));
}

/// Invalid JSON returns error.
#[test]
fn vmess_invalid_json_returns_error() {
    let encoded = base64::engine::general_purpose::STANDARD.encode(b"not json");
    let uri = format!("vmess://{encoded}");
    let err = deve_sub_protocol::parse_uri(&uri).expect_err("should fail");
    assert!(matches!(err, deve_sub_protocol::ParseError::InvalidJson(_)));
}
