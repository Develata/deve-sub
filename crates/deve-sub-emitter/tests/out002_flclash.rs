//! OUT-002: FlClash YAML round-trip validation.
//!
//! FlClash is a Flutter GUI for Clash Meta (Mihomo). It consumes the
//! same Mihomo YAML `proxies:` format as OUT-001, but has no CLI
//! validation command. We validate via round-trip (constraint #18):
//! emit_mihomo → parse_mihomo_yaml → compare semantic equality.

#![allow(clippy::expect_used)]

mod common;

use deve_sub_compatibility::ProfileKind;

/// OUT-002: emitted Mihomo YAML round-trips through the Mihomo parser
/// with semantic equality for every supported protocol.
#[test]
fn out002_flclash_mihomo_yaml_round_trip() {
    let nodes = common::compatible_nodes(ProfileKind::Mihomo);
    assert!(!nodes.is_empty(), "should have compatible nodes");

    let emitted = deve_sub_emitter::emit_mihomo(&nodes).expect("emit mihomo");
    let reparsed =
        deve_sub_protocol::container::parse_mihomo_yaml(&emitted).expect("parse mihomo yaml");

    assert_eq!(nodes.len(), reparsed.len(), "node count should match");
    for (orig, rep) in nodes.iter().zip(reparsed.iter()) {
        assert_eq!(orig.protocol, rep.protocol, "protocol mismatch");
        assert_eq!(orig.endpoint, rep.endpoint, "endpoint mismatch");
        assert_eq!(orig.authentication, rep.authentication, "auth mismatch");
    }
}

/// OUT-002 (negative): malformed YAML is rejected by the parser.
#[test]
fn out002_flclash_mihomo_yaml_rejects_garbage() {
    let err = deve_sub_protocol::container::parse_mihomo_yaml("proxies: [invalid yaml ]]")
        .expect_err("should reject garbage");
    let _ = err;
}
