//! Round-trip golden test for Shadowsocks: parse → emit → compare.
//!
//! Covers PARSE-010 (Base64 padding) and Shadowsocks round-trip as part of
//! PARSE-017.

#![allow(clippy::expect_used)]

use base64::Engine;
use deve_sub_domain::{Authentication, ProtocolConfig, ProtocolKind, ShadowsocksConfig};

/// SIP002 format: parse with Base64URL userinfo.
#[test]
fn shadowsocks_sip002_parse_field_fidelity() {
    // Base64URL("aes-256-gcm:TEST_PASSWORD") without padding.
    let userinfo =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"aes-256-gcm:TEST_PASSWORD");
    let uri = format!("ss://{userinfo}@example.com:8388/?plugin=obfs-local;obfs=http#SS-Test");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");

    assert_eq!(node.protocol, ProtocolKind::Shadowsocks);
    assert_eq!(node.display_name, "SS-Test");

    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password authentication");
    };
    assert_eq!(password, "TEST_PASSWORD");

    assert_eq!(node.endpoint.host.uri_host(), "example.com");
    assert_eq!(node.endpoint.port, 8388);

    let ProtocolConfig::Shadowsocks(ShadowsocksConfig {
        method,
        plugin,
        plugin_opts,
    }) = &node.config
    else {
        panic!("expected Shadowsocks config");
    };
    assert_eq!(method, "aes-256-gcm");
    assert_eq!(plugin.as_deref(), Some("obfs-local"));
    assert_eq!(plugin_opts.as_deref(), Some("obfs=http"));
}

/// Full round-trip: parse → emit → parse → compare nodes.
#[test]
fn shadowsocks_round_trip_semantic() {
    let userinfo = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(b"chacha20-ietf-poly1305:TEST_PASSWORD");
    let uri = format!("ss://{userinfo}@example.com:8388#SS-RT");
    let parsed1 = deve_sub_protocol::parse_uri(&uri).expect("parse 1");
    let emitted = deve_sub_emitter::emit_uri(&parsed1).expect("emit");
    let parsed2 = deve_sub_protocol::parse_uri(&emitted).expect("parse 2");

    assert_eq!(parsed1.protocol, parsed2.protocol);
    assert_eq!(parsed1.config, parsed2.config);
    assert_eq!(parsed1.endpoint, parsed2.endpoint);
    assert_eq!(parsed1.authentication, parsed2.authentication);
    assert_eq!(parsed1.display_name, parsed2.display_name);
}

/// PARSE-010: Base64 with padding also works.
#[test]
fn shadowsocks_sip002_with_padding() {
    // Base64URL with padding. "aes-256-gcm:TEST_PASSWORD" → padded.
    let userinfo = base64::engine::general_purpose::URL_SAFE.encode(b"aes-256-gcm:TEST_PASSWORD");
    let uri = format!("ss://{userinfo}@example.com:8388#SS-Pad");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    let ProtocolConfig::Shadowsocks(cfg) = &node.config else {
        panic!("expected Shadowsocks");
    };
    assert_eq!(cfg.method, "aes-256-gcm");
}

/// PARSE-010: Legacy Base64 format (no `@` in authority, entire body is
/// Base64).
#[test]
fn shadowsocks_legacy_format() {
    let legacy_body = base64::engine::general_purpose::STANDARD
        .encode(b"aes-256-gcm:TEST_PASSWORD@example.com:8388");
    let uri = format!("ss://{legacy_body}#SS-Legacy");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");

    assert_eq!(node.protocol, ProtocolKind::Shadowsocks);
    let Authentication::Password { password } = &node.authentication else {
        panic!("expected Password");
    };
    assert_eq!(password, "TEST_PASSWORD");
    assert_eq!(node.endpoint.host.uri_host(), "example.com");
    assert_eq!(node.endpoint.port, 8388);

    let ProtocolConfig::Shadowsocks(cfg) = &node.config else {
        panic!("expected Shadowsocks");
    };
    assert_eq!(cfg.method, "aes-256-gcm");
}

/// No plugin: emitted URI has no `/?plugin=` part.
#[test]
fn shadowsocks_no_plugin() {
    let userinfo =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"aes-256-gcm:TEST_PASSWORD");
    let uri = format!("ss://{userinfo}@example.com:8388#NoPlugin");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    assert!(!emitted.contains("plugin="));
}

/// Plugin round-trips.
#[test]
fn shadowsocks_plugin_round_trip() {
    let userinfo =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"aes-256-gcm:TEST_PASSWORD");
    let uri = format!("ss://{userinfo}@example.com:8388/?plugin=obfs-local;obfs=http#Plugin");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    let ProtocolConfig::Shadowsocks(cfg) = &node.config else {
        panic!("expected Shadowsocks");
    };
    assert_eq!(cfg.plugin.as_deref(), Some("obfs-local"));
    assert_eq!(cfg.plugin_opts.as_deref(), Some("obfs=http"));

    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(reparsed.config, node.config);
}

/// IPv6 host round-trip.
#[test]
fn shadowsocks_ipv6_round_trip() {
    let userinfo =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"aes-256-gcm:TEST_PASSWORD");
    let uri = format!("ss://{userinfo}@[2001:db8::1]:8388#IPv6");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    assert_eq!(node.endpoint.host.uri_host(), "[2001:db8::1]");
    let emitted = deve_sub_emitter::emit_uri(&node).expect("emit");
    let reparsed = deve_sub_protocol::parse_uri(&emitted).expect("reparse");
    assert_eq!(node.endpoint, reparsed.endpoint);
}

/// Invalid Base64 returns error.
#[test]
fn shadowsocks_invalid_base64_returns_error() {
    let uri = "ss://!!!invalid-base64!!!@example.com:8388#Test";
    let err = deve_sub_protocol::parse_uri(uri).expect_err("should fail");
    assert!(matches!(
        err,
        deve_sub_protocol::ParseError::InvalidBase64(_)
    ));
}

/// Regression: legacy Base64 body containing `/` must not be truncated at
/// the first `/`. Standard Base64 alphabet includes `/`.
#[test]
fn shadowsocks_legacy_base64_with_slash() {
    // "aes-256-gcm:????@h:8" — the `?` chars produce `/` in standard Base64.
    let plaintext = b"aes-256-gcm:????@h:8";
    let encoded = base64::engine::general_purpose::STANDARD.encode(plaintext);
    assert!(
        encoded.contains('/'),
        "test fixture must contain '/' in base64, got: {encoded}"
    );
    let uri = format!("ss://{encoded}#Slash-Test");
    let node = deve_sub_protocol::parse_uri(&uri).expect("parse");
    assert_eq!(node.protocol, ProtocolKind::Shadowsocks);
    assert_eq!(node.endpoint.host.uri_host(), "h");
    assert_eq!(node.endpoint.port, 8);
    let ProtocolConfig::Shadowsocks(cfg) = &node.config else {
        panic!("expected Shadowsocks");
    };
    assert_eq!(cfg.method, "aes-256-gcm");
}
