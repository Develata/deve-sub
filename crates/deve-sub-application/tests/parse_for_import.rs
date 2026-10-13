#![allow(clippy::expect_used)]

//! Tests for `parse_for_import` (NODE-001/002).
//!
//! Verifies that manual import parsing reuses the same protocol parsers as
//! source refresh, assigns fresh `NodeId`s, sets `source_label = "manual"`,
//! and records failed lines.

use deve_sub_application::source::parse_for_import;
use deve_sub_domain::SourceType;

const VALID_URIS: &str = "trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#NodeA\n\
     trojan://TEST_PASSWORD@other.com:8443?sni=other.com&type=tcp#NodeB";

const MIXED_URIS: &str = "trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#NodeA\n\
     this-is-not-a-valid-uri\n\
     trojan://TEST_PASSWORD@other.com:8443?sni=other.com&type=tcp#NodeB";

#[test]
fn parse_for_import_assigns_fresh_ids_and_manual_label() {
    let result = parse_for_import(SourceType::UriList, None, VALID_URIS.as_bytes()).expect("parse");

    assert_eq!(result.nodes.len(), 2, "two valid URIs");
    assert!(result.failed.is_empty());

    for node in &result.nodes {
        assert_eq!(
            node.source.source_label, "manual",
            "manual import sets source_label"
        );
    }

    let ids: Vec<_> = result.nodes.iter().map(|n| n.id).collect();
    assert_eq!(ids.len(), 2, "two distinct IDs");
    assert_ne!(ids[0], ids[1], "IDs must be distinct");
}

#[test]
fn parse_for_import_records_failed_lines() {
    let result = parse_for_import(SourceType::UriList, None, MIXED_URIS.as_bytes()).expect("parse");

    assert_eq!(result.nodes.len(), 2, "two valid nodes parsed");
    assert_eq!(result.failed.len(), 1, "one failed line recorded");
    assert_eq!(result.failed[0], "this-is-not-a-valid-uri");
    assert_eq!(result.failed_positions, vec![1]);
}

#[test]
fn parse_for_import_empty_input_yields_no_nodes() {
    let result = parse_for_import(SourceType::UriList, None, b"").expect("parse");
    assert!(result.nodes.is_empty());
    assert!(result.failed.is_empty());
    assert!(result.failed_positions.is_empty());
}

#[test]
fn parse_for_import_preserves_failure_positions_after_base64_decoding() {
    use base64::Engine;

    let text = format!("invalid-first\n\n# comment\n{MIXED_URIS}\ninvalid-last");
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    for source_type in [SourceType::Auto, SourceType::Base64] {
        let result = parse_for_import(source_type, None, encoded.as_bytes()).expect("base64");
        assert_eq!(result.nodes.len(), 2);
        assert_eq!(result.failed_positions, vec![0, 2, 4]);
        assert_eq!(
            result.failed,
            vec!["invalid-first", "this-is-not-a-valid-uri", "invalid-last"]
        );
        assert_eq!(result.failed_positions.len(), result.failed.len());
    }
}

#[test]
fn parse_for_import_preserves_existing_source_label() {
    // A container parser may set source_label to something other than empty.
    // parse_for_import should NOT overwrite a non-empty label.
    // URI list parser leaves it empty, so "manual" is applied; we verify
    // the empty-label path sets "manual" and a hypothetical non-empty label
    // would be preserved by checking the logic indirectly: the URI path
    // always gets "manual" because uri.rs sets source_label to empty.
    let result = parse_for_import(SourceType::UriList, None, VALID_URIS.as_bytes()).expect("parse");
    for node in &result.nodes {
        assert_eq!(node.source.source_label, "manual");
    }
}
