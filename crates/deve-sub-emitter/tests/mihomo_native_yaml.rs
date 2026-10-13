#![allow(clippy::expect_used)]

//! GEN-013/014: native routing and generated proxies share one YAML document
//! without reparsing generated output or changing scalar/string semantics.

mod common;

use deve_sub_domain::Authentication;
use deve_sub_emitter::{AssembledTemplate, emit_mihomo_full};
use deve_sub_yaml::Value;

fn template(config: &str) -> AssembledTemplate {
    let mut nodes = common::sample_nodes();
    nodes.truncate(1);
    AssembledTemplate {
        clash: Some(config.into()),
        nodes,
        groups: vec![],
        rules: vec![],
        dns: serde_json::Value::Null,
        tun: serde_json::Value::Null,
        output: serde_json::Value::Null,
    }
}

fn parse_native_output(output: &str) -> Value {
    assert!(output.ends_with('\n'), "native output ends with LF");
    assert_eq!(output.lines().filter(|line| *line == "proxies:").count(), 1);
    // The bounded reader rejects a second document or a second root value.
    deve_sub_yaml::from_str(output).expect("one valid YAML document")
}

#[test]
fn empty_native_mapping_emits_one_proxies_root() {
    for config in ["{}", "proxies: [{name: stale, type: unsupported}]"] {
        let ir = template(config);
        let output = emit_mihomo_full(&ir).expect("emit empty native mapping");
        assert!(output.starts_with("proxies:"));
        let value = parse_native_output(&output);
        assert_eq!(value.as_mapping().expect("root mapping").len(), 1);
        assert_eq!(value["proxies"].as_sequence().expect("proxies").len(), 1);
        assert_eq!(
            value["proxies"][0]["name"],
            ir.nodes[0].display_name.as_str()
        );
    }
}

#[test]
fn native_proxies_are_replaced_once_preserving_other_root_key_order() {
    let ir = template(
        "rules: [MATCH,DIRECT]\nproxies: [{name: stale}]\ndns: {enable: true}\nproxy-groups: []\ntun: {enable: false}\n",
    );
    let output = emit_mihomo_full(&ir).expect("emit native mapping");
    let value = parse_native_output(&output);
    let keys: Vec<_> = value
        .as_mapping()
        .expect("mapping")
        .keys()
        .map(|key| key.as_str().expect("string key"))
        .collect();
    assert_eq!(keys, vec!["rules", "dns", "proxy-groups", "tun", "proxies"]);
    assert_eq!(value["proxies"].as_sequence().expect("proxies").len(), 1);
    assert_eq!(value["proxies"][0]["name"], "trojan-test");
    assert!(!output.contains("stale"));
}

#[test]
fn trailing_native_block_scalars_remain_separate_from_proxies() {
    for style in ["|-", "|", "|+", ">", ">+"] {
        let config = format!("dns:\n  fixture: {style}\n    first\n    proxies: fixture\n\n");
        let expected = deve_sub_yaml::parse_template(&config).expect("native scalar");
        let output = emit_mihomo_full(&template(&config)).expect("emit scalar");
        let actual = parse_native_output(&output);
        assert_eq!(actual["dns"], expected["dns"], "block style {style}");
        assert_eq!(actual["proxies"].as_sequence().expect("proxies").len(), 1);
    }
}

#[test]
fn native_zero_nodes_keep_null_proxies() {
    for config in ["{}", "rules: [MATCH,DIRECT]\n"] {
        let mut ir = template(config);
        ir.nodes.clear();
        let output = emit_mihomo_full(&ir).expect("emit zero nodes");
        let value = parse_native_output(&output);
        assert_eq!(value.get("proxies"), Some(&Value::Null));
    }
}

#[test]
fn native_and_generated_strings_round_trip_without_interpolation() {
    for text in [
        "00123",
        "true",
        "false",
        "null",
        "2026-10-13",
        "${DEVE_SUB_TEST_VALUE}",
        "${{ env.DEVE_SUB_TEST_VALUE }}",
        "control\0\u{7}\u{8}\t\n\u{b}\u{c}\r\u{1b}\u{85}\u{2028}\u{2029}",
        "trailing\n\n",
    ] {
        let quoted = serde_json::to_string(text)
            .expect("quote string")
            .replace('\u{85}', "\\u0085")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
        let mut ir = template(&format!("dns:\n  \"1\": {quoted}\n"));
        ir.nodes[0].display_name = text.into();
        ir.nodes[0].authentication = Authentication::Password {
            password: text.into(),
        };
        let output = emit_mihomo_full(&ir).expect("emit strings");
        assert!(!output.contains(['\u{85}', '\u{2028}', '\u{2029}']));
        assert_eq!(
            deve_sub_yaml::inspect_generated_sequence(&output, "proxies")
                .expect("publication validates emitted strings"),
            deve_sub_yaml::SequenceState::NonEmpty
        );
        let value = parse_native_output(&output);
        assert_eq!(value["dns"]["1"].as_str(), Some(text));
        assert_eq!(value["proxies"][0]["name"].as_str(), Some(text));
        assert_eq!(value["proxies"][0]["password"].as_str(), Some(text));
        ir.clash = None;
        let v3 = emit_mihomo_full(&ir).expect("emit V3 strings");
        assert_eq!(
            deve_sub_yaml::inspect_generated_sequence(&v3, "proxies")
                .expect("V3 publication validates emitted strings"),
            deve_sub_yaml::SequenceState::NonEmpty
        );
        let value = deve_sub_yaml::parse_generated_value(&v3).expect("V3 output");
        assert_eq!(value["proxies"][0]["name"].as_str(), Some(text));
        assert_eq!(value["proxies"][0]["password"].as_str(), Some(text));
    }
}

#[test]
fn generated_proxies_are_not_subject_to_yaml_input_limits() {
    let mut ir = template("dns: {enable: true}\n");
    ir.nodes[0].display_name = "x".repeat(deve_sub_yaml::Limits::SOURCE.input_bytes + 1);
    let output = emit_mihomo_full(&ir).expect("emit output larger than a source input");
    assert!(output.len() > deve_sub_yaml::Limits::SOURCE.input_bytes);
    assert_eq!(output.lines().filter(|line| *line == "proxies:").count(), 1);
    assert!(output.ends_with('\n'));
}

#[test]
fn expanded_native_groups_are_not_subject_to_authored_template_limits() {
    let authored = "proxy-groups: []\nrules: [MATCH,DIRECT]\n";
    let mut native = deve_sub_yaml::parse_template(authored).expect("small authored template");
    let members: Vec<_> = (0..10_000)
        .map(|index| format!("node-{index:05}-{}", "x".repeat(32)))
        .collect();
    let groups: Vec<_> = (0..3)
        .map(|index| {
            serde_json::json!({
                "name": format!("fixture-select-{index}"),
                "type": "select",
                "proxies": members,
            })
        })
        .collect();
    // WHY: server group expansion happens after authored-template validation
    // and can produce a valid native configuration larger than its input cap.
    native.as_mapping_mut().expect("native mapping").insert(
        Value::String("proxy-groups".into()),
        deve_sub_yaml::from_json(serde_json::json!(groups)),
    );
    let expanded = deve_sub_yaml::to_string(&native).expect("serialize expanded routing");
    assert!(expanded.len() > deve_sub_yaml::Limits::TEMPLATE.input_bytes);
    assert!(deve_sub_yaml::parse_template(&expanded).is_err());

    let output = emit_mihomo_full(&template(&expanded)).expect("emit expanded native routing");
    let actual = parse_native_output(&output);
    assert_eq!(actual["proxy-groups"], native["proxy-groups"]);
    assert_eq!(actual["rules"], native["rules"]);
    assert_eq!(actual["proxies"].as_sequence().expect("proxies").len(), 1);
}

#[test]
fn v3_rules_dns_and_tun_keep_string_scalars() {
    let strings = serde_json::json!([
        "00123",
        "true",
        "null",
        "2026-10-13",
        "${DEVE_SUB_TEST_VALUE}",
        "${{ env.DEVE_SUB_TEST_VALUE }}",
        "control\u{0}\t\n\r\u{1b}",
        "trailing\n\n"
    ]);
    let mut ir = template("{}");
    ir.clash = None;
    ir.rules = vec![serde_json::json!({ "fixture": strings })];
    ir.dns = serde_json::json!({ "fixture": strings });
    ir.tun = serde_json::json!({ "fixture": strings });
    let output = emit_mihomo_full(&ir).expect("emit V3 opaque fields");
    let value = deve_sub_yaml::parse_source_json(&output).expect("parse V3 output");
    assert_eq!(value["rules"], serde_json::json!(ir.rules));
    assert_eq!(value["dns"], ir.dns);
    assert_eq!(value["tun"], ir.tun);
}
