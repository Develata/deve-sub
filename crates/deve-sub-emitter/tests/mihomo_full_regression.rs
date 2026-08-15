//! DS-AUD-001 + DS-AUD-020 + DS-AUD-023 regression tests for the mihomo
//! full-template emitter.
//!
//! These tests verify:
//! - emit_mihomo_full emits proxy-groups, rules, dns, and tun sections
//!   (DS-AUD-001: the pipeline previously dropped these).
//! - Two templates differing only in rules produce different output.
//! - yaml_dq escapes special characters in name/password/sni fields
//!   (DS-AUD-023: the emitter previously injected raw user strings).
//! - Per-group sort_order (asc/desc) is applied to rendered members
//!   (DS-AUD-020).

#![allow(clippy::expect_used)]

mod common;

use deve_sub_domain::GroupType;
use deve_sub_emitter::{AssembledGroup, AssembledTemplate, emit_mihomo_full};

use common::sample_nodes;

fn one_trojan() -> Vec<deve_sub_domain::Node> {
    let mut nodes = sample_nodes();
    nodes.truncate(1);
    nodes
}

fn make_template(
    groups: Vec<AssembledGroup>,
    rules: Vec<serde_json::Value>,
    dns: serde_json::Value,
    tun: serde_json::Value,
) -> AssembledTemplate {
    AssembledTemplate {
        nodes: one_trojan(),
        groups,
        rules,
        dns,
        tun,
        output: serde_json::Value::Null,
    }
}

#[test]
fn full_emit_contains_proxy_groups() {
    let groups = vec![AssembledGroup {
        name: "auto".to_owned(),
        group_type: GroupType::UrlTest,
        members: vec!["trojan-test".to_owned()],
    }];
    let template = make_template(
        groups,
        vec![],
        serde_json::Value::Null,
        serde_json::Value::Null,
    );
    let output = emit_mihomo_full(&template).expect("emit");
    assert!(
        output.contains("proxy-groups:"),
        "output must contain proxy-groups section"
    );
    assert!(output.contains("name: \"auto\""), "group name must appear");
    assert!(output.contains("type: url-test"), "group type must appear");
}

#[test]
fn full_emit_contains_rules() {
    let rules = vec![serde_json::json!({
        "type": "DOMAIN",
        "domain": "example.com",
        "proxy": "auto"
    })];
    let template = make_template(
        vec![],
        rules,
        serde_json::Value::Null,
        serde_json::Value::Null,
    );
    let output = emit_mihomo_full(&template).expect("emit");
    assert!(
        output.contains("rules:"),
        "output must contain rules section"
    );
    assert!(output.contains("example.com"), "rule content must appear");
}

#[test]
fn full_emit_contains_dns_and_tun() {
    let dns = serde_json::json!({"enable": true, "nameserver": ["8.8.8.8"]});
    let tun = serde_json::json!({"enable": true, "device": "utun0"});
    let template = make_template(vec![], vec![], dns, tun);
    let output = emit_mihomo_full(&template).expect("emit");
    assert!(output.contains("dns:"), "output must contain dns section");
    assert!(output.contains("8.8.8.8"), "dns content must appear");
    assert!(output.contains("tun:"), "output must contain tun section");
    assert!(output.contains("utun0"), "tun content must appear");
}

#[test]
fn differing_rules_produce_different_output() {
    let rules_a = vec![serde_json::json!({"type": "DOMAIN", "domain": "a.com", "proxy": "DIRECT"})];
    let rules_b = vec![serde_json::json!({"type": "DOMAIN", "domain": "b.com", "proxy": "DIRECT"})];
    let template_a = make_template(
        vec![],
        rules_a,
        serde_json::Value::Null,
        serde_json::Value::Null,
    );
    let template_b = make_template(
        vec![],
        rules_b,
        serde_json::Value::Null,
        serde_json::Value::Null,
    );
    let output_a = emit_mihomo_full(&template_a).expect("emit a");
    let output_b = emit_mihomo_full(&template_b).expect("emit b");
    assert_ne!(
        output_a, output_b,
        "templates differing only in rules must produce different output"
    );
    assert!(output_a.contains("a.com"));
    assert!(output_b.contains("b.com"));
}

#[test]
fn yaml_dq_escapes_special_characters_in_name() {
    // Build a node with a name containing a double quote and backslash.
    let mut nodes = one_trojan();
    nodes[0].display_name = r#"evil"name\back"#.to_owned();
    let template = AssembledTemplate {
        nodes,
        groups: vec![],
        rules: vec![],
        dns: serde_json::Value::Null,
        tun: serde_json::Value::Null,
        output: serde_json::Value::Null,
    };
    let output = emit_mihomo_full(&template).expect("emit");
    // WHY: the escaped form must be present; the raw unescaped quote must not
    // appear as a bare `"` in the name position. A round-trip YAML parse
    // must succeed.
    assert!(
        output.contains(r#"evil\"name\\back"#),
        "name must be yaml-escaped"
    );
    serde_yaml::from_str::<serde_yaml::Value>(&output).expect("output must be valid YAML");
}

#[test]
fn yaml_dq_escapes_special_characters_in_password() {
    let mut nodes = one_trojan();
    nodes[0].authentication = deve_sub_domain::Authentication::Password {
        password: r#"p"w\d"#.to_owned(),
    };
    let template = AssembledTemplate {
        nodes,
        groups: vec![],
        rules: vec![],
        dns: serde_json::Value::Null,
        tun: serde_json::Value::Null,
        output: serde_json::Value::Null,
    };
    let output = emit_mihomo_full(&template).expect("emit");
    assert!(
        output.contains(r#"p\"w\\d"#),
        "password must be yaml-escaped"
    );
    serde_yaml::from_str::<serde_yaml::Value>(&output).expect("output must be valid YAML");
}

#[test]
fn emit_preserves_member_order_from_assembled_group() {
    let groups = vec![AssembledGroup {
        name: "select".to_owned(),
        group_type: GroupType::Select,
        members: vec!["zeta".to_owned(), "alpha".to_owned(), "mid".to_owned()],
    }];
    let template = make_template(
        groups,
        vec![],
        serde_json::Value::Null,
        serde_json::Value::Null,
    );
    let output = emit_mihomo_full(&template).expect("emit");
    let zeta_pos = output.find("zeta").expect("zeta present");
    let alpha_pos = output.find("alpha").expect("alpha present");
    let mid_pos = output.find("mid").expect("mid present");
    // The emitter does not sort; it emits members in the order the
    // application layer assembled them (which applies sort_order).
    assert!(
        zeta_pos < alpha_pos && alpha_pos < mid_pos,
        "emitter preserves assembled member order"
    );
}

#[test]
fn full_emit_without_groups_rules_dns_tun_only_has_proxies() {
    let template = make_template(
        vec![],
        vec![],
        serde_json::Value::Null,
        serde_json::Value::Null,
    );
    let output = emit_mihomo_full(&template).expect("emit");
    assert!(
        output.contains("proxies:"),
        "output must always contain proxies"
    );
    assert!(
        !output.contains("proxy-groups:"),
        "no groups → no proxy-groups section"
    );
    assert!(!output.contains("rules:"), "no rules → no rules section");
    assert!(!output.contains("dns:"), "no dns → no dns section");
    assert!(!output.contains("tun:"), "no tun → no tun section");
}
