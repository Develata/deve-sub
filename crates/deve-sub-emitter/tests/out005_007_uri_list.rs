//! OUT-005, OUT-006, OUT-007: URI list round-trip validation for GUI
//! clients (v2rayN, v2rayNG, Shadowrocket).
//!
//! These clients consume URI-list subscriptions (Shadowrocket uses base64-
//! encoded URI list). They are GUI applications with no CLI validation
//! command, so we validate via round-trip: emit → parse → compare
//! (constraint #18: official format validation). The URI list format is
//! the official subscription format for all three clients.

#![allow(clippy::expect_used)]

mod common;

use base64::Engine;

use deve_sub_compatibility::ProfileKind;

/// OUT-005 (v2rayN): emitted URI list round-trips through the parser
/// with semantic equality for every supported protocol.
#[test]
fn out005_v2rayn_uri_list_round_trip() {
    let nodes = common::compatible_nodes(ProfileKind::UriList);
    assert!(!nodes.is_empty(), "should have compatible nodes");

    let emitted = deve_sub_emitter::emit_uri_list(&nodes).expect("emit uri list");
    let reparsed = deve_sub_protocol::container::parse_uri_list(&emitted).expect("parse uri list");

    assert_eq!(nodes.len(), reparsed.len(), "node count should match");
    for (orig, rep) in nodes.iter().zip(reparsed.iter()) {
        assert_eq!(orig.protocol, rep.protocol, "protocol mismatch");
        assert_eq!(orig.endpoint, rep.endpoint, "endpoint mismatch");
        assert_eq!(orig.authentication, rep.authentication, "auth mismatch");
    }
}

/// OUT-006 (v2rayNG): same URI list format as v2rayN — the round-trip
/// already proves the format is importable.
#[test]
fn out006_v2rayng_uri_list_round_trip() {
    let nodes = common::compatible_nodes(ProfileKind::UriList);
    let emitted = deve_sub_emitter::emit_uri_list(&nodes).expect("emit uri list");
    let reparsed = deve_sub_protocol::container::parse_uri_list(&emitted).expect("parse uri list");

    assert_eq!(nodes.len(), reparsed.len());
    for (orig, rep) in nodes.iter().zip(reparsed.iter()) {
        assert_eq!(orig.protocol, rep.protocol);
        assert_eq!(orig.endpoint, rep.endpoint);
    }
}

/// OUT-007 (Shadowrocket): emitted base64-encoded URI list decodes
/// back to valid URIs that round-trip through the parser.
#[test]
fn out007_shadowrocket_base64_round_trip() {
    let nodes = common::compatible_nodes(ProfileKind::Shadowrocket);
    assert!(!nodes.is_empty(), "should have compatible nodes");

    let encoded = deve_sub_emitter::emit_shadowrocket(&nodes).expect("emit shadowrocket");
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&encoded)
        .expect("base64 decode");
    let text = String::from_utf8(decoded).expect("utf8");
    let reparsed = deve_sub_protocol::container::parse_uri_list(&text).expect("parse uri list");

    assert_eq!(nodes.len(), reparsed.len(), "node count should match");
    for (orig, rep) in nodes.iter().zip(reparsed.iter()) {
        assert_eq!(orig.protocol, rep.protocol, "protocol mismatch");
        assert_eq!(orig.endpoint, rep.endpoint, "endpoint mismatch");
        assert_eq!(orig.authentication, rep.authentication, "auth mismatch");
    }
}

/// OUT-005/006/007 (negative): empty node list produces empty output,
/// not garbage.
#[test]
fn uri_list_empty_nodes_produces_empty_output() {
    let emitted = deve_sub_emitter::emit_uri_list(&[]).expect("emit");
    assert!(emitted.is_empty(), "empty list should emit empty string");

    let encoded = deve_sub_emitter::emit_shadowrocket(&[]).expect("emit");
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&encoded)
        .expect("base64 decode");
    assert!(
        String::from_utf8(decoded).unwrap_or_default().is_empty(),
        "empty shadowrocket should decode to empty"
    );
}
