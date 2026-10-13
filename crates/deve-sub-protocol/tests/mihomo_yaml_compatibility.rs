#![allow(clippy::expect_used)]

//! PARSE-005: YAML source imports retain the historical JSON projection,
//! independently of the stricter ordered template representation.

use deve_sub_domain::{ProtocolConfig, ProtocolKind};
use deve_sub_protocol::container::parse_mihomo_yaml;

#[test]
fn source_scalar_keys_duplicates_and_merge_keys_keep_json_semantics() {
    let input = r#"
defaults: &defaults
  server: example.com
proxies:
  - type: replaced-protocol
    type: future-protocol
    <<: *defaults
    001: leading-zero
    1.0: decimal-key
    true: boolean-key
    null: null-key
    on: plain-word
    duplicate: first
    duplicate: last
"#;
    let nodes = parse_mihomo_yaml(input).expect("source YAML");
    assert_eq!(nodes.len(), 1);
    assert_eq!(
        nodes[0].protocol,
        ProtocolKind::Unknown("future-protocol".into())
    );
    let ProtocolConfig::Unsupported(node) = &nodes[0].config else {
        panic!("unknown source entry must be preserved");
    };
    assert_eq!(node.raw_format.as_deref(), Some("mihomo-yaml"));
    assert_eq!(node.raw["001"], "leading-zero");
    assert_eq!(node.raw["1.0"], "decimal-key");
    assert_eq!(node.raw["true"], "boolean-key");
    assert_eq!(node.raw["null"], "null-key");
    assert_eq!(node.raw["on"], "plain-word");
    assert_eq!(node.raw["duplicate"], "last");
    assert_eq!(node.raw["<<"]["server"], "example.com");
    assert!(
        node.raw.get("server").is_none(),
        "source merge keys stay literal"
    );
}

#[test]
fn source_quoted_scalars_stay_strings_without_interpolation() {
    for value in [
        "00123",
        "true",
        "null",
        "2026-10-13",
        "${DEVE_SUB_TEST_VALUE}",
        "${{ env.DEVE_SUB_TEST_VALUE }}",
        "control\0\u{7}\u{8}\t\n\u{b}\u{c}\r\u{1b}\u{85}\u{2028}\u{2029}",
    ] {
        let quoted = serde_json::to_string(value)
            .expect("quote scalar")
            .replace('\u{85}', "\\u0085")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
        let input = format!("proxies:\n  - type: future-protocol\n    fixture: {quoted}\n");
        let nodes = parse_mihomo_yaml(&input).expect("source scalar");
        let ProtocolConfig::Unsupported(node) = &nodes[0].config else {
            panic!("unknown source entry must be preserved");
        };
        assert_eq!(node.raw["fixture"].as_str(), Some(value));
    }
}
