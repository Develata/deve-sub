//! Golden tests for URI list, Base64 subscription, and Shadowrocket
//! share list parsers (PARSE-009, PARSE-010, PARSE-016).

#![allow(clippy::expect_used)]

use base64::Engine;
use deve_sub_domain::{ProtocolConfig, ProtocolKind};

const RESERVED_UUID: &str = "00000000-0000-4000-8000-000000000001";
const RESERVED_PASSWORD: &str = "TEST_PASSWORD";

// --- URI list ---

/// PARSE-009: URI list with mixed protocols and IPv6.
#[test]
fn uri_list_parses_multiple_protocols() {
    let text = format!(
        "trojan://{RESERVED_PASSWORD}@example.com:443?type=tcp#Trojan-Node\n\
         vless://{RESERVED_UUID}@[2001:db8::1]:443?security=reality&type=tcp&pbk=TEST_PUBLIC_KEY&sid=01020304&sni=example.com#IPv6-Node\n\
         # comment line\n\
         \n\
         ss://YWVzLTI1Ni1nY206dGVzdC1wYXNzd29yZA==@ss.example.com:8388#SS-Node"
    );

    let nodes = deve_sub_protocol::container::parse_uri_list(&text).expect("parse");

    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0].protocol, ProtocolKind::Trojan);
    assert_eq!(nodes[1].protocol, ProtocolKind::Vless);
    assert_eq!(nodes[1].endpoint.host.uri_host(), "[2001:db8::1]");
    assert_eq!(nodes[2].protocol, ProtocolKind::Shadowsocks);
}

/// URI list preserves unknown-scheme URIs as UnsupportedNode (constraint #7).
#[test]
fn uri_list_preserves_unknown_scheme() {
    let text = "unknownproto://test@host.com:443#Unknown";
    let nodes = deve_sub_protocol::container::parse_uri_list(text).expect("parse");
    assert_eq!(nodes.len(), 1);
    assert!(matches!(nodes[0].protocol, ProtocolKind::Unknown(_)));
    assert!(matches!(nodes[0].config, ProtocolConfig::Unsupported(_)));
}

/// URI list skips malformed lines silently.
#[test]
fn uri_list_skips_malformed_lines() {
    let text = "not a uri\ntrojan://TEST_PASSWORD@example.com:443?type=tcp#OK\n  \n";
    let nodes = deve_sub_protocol::container::parse_uri_list(text).expect("parse");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].protocol, ProtocolKind::Trojan);
}

/// Empty URI list returns empty vec.
#[test]
fn uri_list_empty_returns_empty_vec() {
    let nodes = deve_sub_protocol::container::parse_uri_list("").expect("parse");
    assert!(nodes.is_empty());
}

// --- Base64 subscription ---

/// Base64 subscription decodes to URI list.
#[test]
fn base64_subscription_decodes_to_nodes() {
    let uris = "trojan://TEST_PASSWORD@example.com:443?type=tcp#Base64-Test\n";
    let encoded = base64::engine::general_purpose::STANDARD.encode(uris.as_bytes());

    let nodes = deve_sub_protocol::container::parse_base64_subscription(&encoded).expect("parse");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].protocol, ProtocolKind::Trojan);
    assert_eq!(nodes[0].display_name, "Base64-Test");
}

/// Base64 subscription without padding also works (PARSE-010).
#[test]
fn base64_subscription_no_padding() {
    let uris = "trojan://TEST_PASSWORD@example.com:443?type=tcp#NoPad\n";
    let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(uris.as_bytes());

    let nodes = deve_sub_protocol::container::parse_base64_subscription(&encoded).expect("parse");
    assert_eq!(nodes.len(), 1);
}

// --- Shadowrocket ---

/// Shadowrocket share list is the same as URI list.
#[test]
fn shadowrocket_parses_share_list() {
    let text = format!(
        "trojan://{RESERVED_PASSWORD}@example.com:443?type=tcp#SR-Trojan\n\
         vless://{RESERVED_UUID}@[2001:db8::1]:443?security=reality&type=tcp&pbk=TEST_PUBLIC_KEY&sid=01020304#SR-VLESS"
    );

    let nodes = deve_sub_protocol::container::parse_shadowrocket(&text).expect("parse");
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].protocol, ProtocolKind::Trojan);
    assert_eq!(nodes[1].protocol, ProtocolKind::Vless);
    assert_eq!(nodes[1].endpoint.host.uri_host(), "[2001:db8::1]");
}

// --- URI list emission (PARSE-016) ---

/// PARSE-016: emit_uri_list produces one URI per line with LF endings.
#[test]
fn emit_uri_list_one_per_line() {
    let uri1 = "trojan://TEST_PASSWORD@example.com:443?type=tcp#Node1";
    let uri2 = "trojan://TEST_PASSWORD@example.com:8443?type=tcp#Node2";

    let node1 = deve_sub_protocol::parse_uri(uri1).expect("parse1");
    let node2 = deve_sub_protocol::parse_uri(uri2).expect("parse2");

    let emitted = deve_sub_emitter::emit_uri_list(&[node1, node2]).expect("emit");
    let lines: Vec<&str> = emitted.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("Node1"));
    assert!(lines[1].contains("Node2"));
}

/// PARSE-016: emit_uri_list skips unsupported nodes.
#[test]
fn emit_uri_list_skips_unsupported() {
    let uri = "trojan://TEST_PASSWORD@example.com:443?type=tcp#Supported";
    let node = deve_sub_protocol::parse_uri(uri).expect("parse");

    let mut unsupported = node.clone();
    unsupported.config = ProtocolConfig::Unsupported(deve_sub_domain::UnsupportedNode {
        raw: serde_json::Value::Null,
        raw_format: None,
        reason: "test".to_owned(),
    });

    let emitted = deve_sub_emitter::emit_uri_list(&[node, unsupported]).expect("emit");
    let lines: Vec<&str> = emitted.lines().collect();
    assert_eq!(lines.len(), 1);
}

/// Round-trip: parse URI list → emit → parse → same count.
#[test]
fn uri_list_round_trip_count() {
    let text = "trojan://TEST_PASSWORD@example.com:443?type=tcp#A\n\
                trojan://TEST_PASSWORD@example.com:8443?type=tcp#B\n\
                vless://00000000-0000-4000-8000-000000000001@[2001:db8::1]:443?security=reality&type=tcp&pbk=TEST_PUBLIC_KEY&sid=01020304#C";

    let nodes = deve_sub_protocol::container::parse_uri_list(text).expect("parse1");
    let emitted = deve_sub_emitter::emit_uri_list(&nodes).expect("emit");
    let reparsed = deve_sub_protocol::container::parse_uri_list(&emitted).expect("parse2");

    assert_eq!(nodes.len(), reparsed.len());
    for (a, b) in nodes.iter().zip(reparsed.iter()) {
        assert_eq!(a.protocol, b.protocol);
        assert_eq!(a.endpoint, b.endpoint);
        assert_eq!(a.authentication, b.authentication);
    }
}
