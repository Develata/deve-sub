#![allow(clippy::expect_used)]

//! PARSE-006/007/008: format detection must not confuse unsupported entries
//! with evidence that a different JSON dialect should handle the document.

use deve_sub_application::source::{ParseContentError, parse_content};
use deve_sub_domain::{ItemParseStatus, ProtocolConfig, ProtocolKind, SourceType};

const XRAY_JSON: &str = include_str!("fixtures/xray-auto.json");

#[test]
fn auto_xray_matches_explicit_trojan_and_vless_parsing() {
    let expected = parse_content(SourceType::XrayJson, None, XRAY_JSON.as_bytes()).expect("Xray");
    assert_eq!(expected.len(), 2);
    assert!(matches!(
        expected[0].node.as_ref().expect("Trojan").config,
        ProtocolConfig::Trojan(_)
    ));
    assert!(matches!(
        expected[1].node.as_ref().expect("VLESS").config,
        ProtocolConfig::VlessReality(_)
    ));

    for content_type in [None, Some("application/json")] {
        let actual = parse_content(SourceType::Auto, content_type, XRAY_JSON.as_bytes())
            .expect("auto detection");
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.into_iter().zip(&expected) {
            assert_eq!(actual.initial_status, ItemParseStatus::Parsed);
            assert_eq!(actual.raw_uri, expected.raw_uri);
            let mut actual = actual.node.expect("detected node");
            let expected = expected.node.as_ref().expect("explicit node");
            actual.id = expected.id;
            actual.source.imported_at = expected.source.imported_at;
            assert_eq!(&actual, expected);
        }
    }
}

#[test]
fn auto_singbox_unknown_and_malformed_nodes_remain_lossless() {
    let outbounds = serde_json::json!([
        { "type": "future-protocol", "tag": "unknown", "server": "example.com" },
        { "type": "trojan", "tag": "missing-password", "server": "example.com" }
    ]);
    let body = serde_json::json!({ "outbounds": outbounds }).to_string();
    let parsed = parse_content(SourceType::Auto, None, body.as_bytes()).expect("sing-box");
    assert_eq!(parsed.len(), 2);
    for (index, entry) in parsed.into_iter().enumerate() {
        assert_eq!(entry.initial_status, ItemParseStatus::Unsupported);
        let ProtocolConfig::Unsupported(node) = entry.node.expect("preserved").config else {
            panic!("unknown and malformed nodes must remain unsupported");
        };
        assert_eq!(node.raw_format.as_deref(), Some("singbox-json"));
        assert_eq!(node.raw, outbounds[index]);
    }
}

#[test]
fn auto_json_type_key_takes_precedence_over_protocol_key() {
    for outbounds in [
        serde_json::json!([{ "type": "future-protocol", "protocol": "trojan" }]),
        serde_json::json!([{ "protocol": "trojan" }, { "type": "future-protocol" }]),
        serde_json::json!([{ "type": null, "protocol": "trojan" }]),
    ] {
        let body = serde_json::json!({ "outbounds": outbounds }).to_string();
        let parsed = parse_content(SourceType::Auto, None, body.as_bytes()).expect("sing-box");
        assert_eq!(parsed.len(), outbounds.as_array().expect("array").len());
        for (index, entry) in parsed.into_iter().enumerate() {
            let ProtocolConfig::Unsupported(node) = entry.node.expect("preserved").config else {
                panic!("ambiguous entries must retain sing-box handling");
            };
            assert_eq!(node.raw_format.as_deref(), Some("singbox-json"));
            assert_eq!(node.raw, outbounds[index]);
        }
    }
}

#[test]
fn auto_xray_unknown_protocol_is_preserved_in_its_own_format() {
    let outbound = serde_json::json!({ "protocol": "future-protocol", "tag": "unknown" });
    let body = serde_json::json!({ "outbounds": [outbound] }).to_string();
    let parsed = parse_content(SourceType::Auto, None, body.as_bytes()).expect("Xray");
    let node = parsed[0].node.as_ref().expect("node");
    assert_eq!(
        node.protocol,
        ProtocolKind::Unknown("future-protocol".into())
    );
    let ProtocolConfig::Unsupported(node) = &node.config else {
        panic!("unknown protocol must remain unsupported");
    };
    assert_eq!(node.raw_format.as_deref(), Some("xray-json"));
    assert_eq!(node.raw, outbound);
}

#[test]
fn auto_json_empty_and_unmarked_entries_keep_singbox_behavior() {
    assert!(
        parse_content(SourceType::Auto, None, br#"{"outbounds":[]}"#)
            .expect("empty")
            .is_empty()
    );
    let parsed =
        parse_content(SourceType::Auto, None, br#"{"outbounds":[{}]}"#).expect("unmarked entry");
    let ProtocolConfig::Unsupported(node) = &parsed[0].node.as_ref().expect("node").config else {
        panic!("unmarked entry must be preserved");
    };
    assert_eq!(node.raw_format.as_deref(), Some("singbox-json"));
    assert_eq!(node.raw, serde_json::json!({}));
}

#[test]
fn auto_json_keeps_container_limits_and_structural_errors() {
    for entry in [r#"{"type":"future"}"#, r#"{"protocol":"future"}"#] {
        let body = format!("{{\"outbounds\":[{}]}}", vec![entry; 10_001].join(","));
        assert!(matches!(
            parse_content(SourceType::Auto, None, body.as_bytes()),
            Err(ParseContentError::TooManyNodes(10_001))
        ));
    }
    for body in ["{", "{}", r#"{"outbounds":{}}"#, "[]"] {
        assert!(parse_content(SourceType::Auto, None, body.as_bytes()).is_err());
    }
}

#[test]
fn auto_json_preserves_document_errors_before_wrapped_entry_limits() {
    let entries = vec![r#"{"protocol":"future"}"#; 10_001].join(",");
    let body = format!("{{\"outbounds\":[{entries}]}} bad");
    let error = parse_content(SourceType::Auto, Some("application/json"), body.as_bytes())
        .expect_err("malformed complete document");
    assert!(matches!(error, ParseContentError::Parse(_)));
    assert_eq!(
        error.to_string(),
        "parse error: invalid JSON: trailing characters at line 1 column 220039"
    );
    let body = format!("{{\"outbounds\":[{entries}]}}");
    assert!(matches!(
        parse_content(SourceType::Auto, Some("application/json"), body.as_bytes()),
        Err(ParseContentError::TooManyNodes(10_001))
    ));
}

#[test]
fn auto_json_keeps_byte_then_utf8_checks_before_dispatch() {
    assert!(matches!(
        parse_content(
            SourceType::Auto,
            Some("application/json"),
            &vec![0xff; 10 * 1024 * 1024 + 1]
        ),
        Err(ParseContentError::TooLarge)
    ));
    assert!(matches!(
        parse_content(SourceType::Auto, Some("application/json"), &[0xff]),
        Err(ParseContentError::InvalidUtf8(_))
    ));
}

#[test]
fn auto_keeps_yaml_heuristics_ahead_of_json_detection() {
    for (content_type, body) in [
        ("application/yaml", r#"{"outbounds":[]}"#),
        ("application/json", r#"{"outbounds":[],"note":"proxies:"}"#),
    ] {
        let error = parse_content(SourceType::Auto, Some(content_type), body.as_bytes())
            .expect_err("existing YAML precedence");
        assert_eq!(
            error.to_string(),
            "parse error: missing container key: proxies"
        );
    }
}
