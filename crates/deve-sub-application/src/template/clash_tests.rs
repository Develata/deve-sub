use super::validation::{parse_template_document, validate_document};

fn check(yaml: &str) -> Result<(), super::error::TemplateAppError> {
    let doc = parse_template_document(yaml)?;
    validate_document(&doc, yaml)
}

#[test]
fn native_clash_default_and_bare_rules_are_valid() {
    check(include_str!(
        "../../../../examples/templates/clash-routing.yaml"
    ))
    .expect("default template");
    let raw = "- DOMAIN-SUFFIX,example.com,DIRECT\n- MATCH,PROXY\n";
    check(raw).expect("bare rule list");
    let doc = parse_template_document(raw).expect("parse");
    assert_eq!(doc.spec.target_profiles, ["mihomo"]);
    let native = super::clash::config(&doc.spec.clash.expect("native")).expect("config");
    assert_eq!(native["proxy-groups"][0]["include-all-proxies"], true);
}

#[test]
fn native_clash_preserves_provider_logical_rules_and_options() {
    let raw = r#"
proxy-groups:
  - {name: PROXY, type: url-test, include-all-proxies: true, filter: '(?i)hk|港', interval: 300, url: 'https://example.com/check'}
rule-providers:
  local: {type: inline, behavior: domain, payload: ['+.example.com']}
rules:
  - RULE-SET,local,DIRECT
  - AND,((DOMAIN-SUFFIX,example.com),(NOT,((NETWORK,UDP)))),PROXY
  - MATCH,PROXY
dns: {enable: true, nameserver: ['https://dns.example.com/dns-query']}
tun: {enable: false}
"#;
    check(raw).expect("native configuration");
    let native = parse_template_document(raw)
        .expect("parse")
        .spec
        .clash
        .expect("native");
    let original: serde_json::Value = serde_yaml::from_str(raw).expect("yaml");
    assert_eq!(
        serde_json::Value::Object(super::clash::config(&native).expect("config")),
        original
    );
}

#[test]
fn native_clash_rejects_invalid_routing_without_silent_drops() {
    for yaml in [
        "rules: [MATCH,PROXY]",
        "rules: ['MATCH,missing']",
        "rules: ['RULE-SET,missing,DIRECT']",
        "rules: ['UNKNOWN,a,DIRECT']",
        "rules: ['AND,((NETWORK,UDP),DIRECT']",
        "rules: ['MATCH,DIRECT,extra']",
        "rules: []",
        "rules: ['MATCH,DIRECT']\nproxies: []",
        "rules: ['MATCH,DIRECT']\nproxy-providers: {}",
        "rules: ['MATCH,DIRECT']\nscripts: {}",
        "rules: ['MATCH,DIRECT']\ndns: []",
        "rules: ['MATCH,DIRECT']\nscript: anything",
        "rules: ['MATCH,DIRECT']\nproxy-groups: [{name: DIRECT, type: select, proxies: [REJECT]}]",
        "rules: ['MATCH,A']\nproxy-groups: [{name: A, type: select, proxies: [B]}, {name: B, type: select, proxies: [A]}]",
        "rules: ['MATCH,A']\nproxy-groups: [{name: A, type: select, include-all-proxies: 'true'}]",
        "rules: ['MATCH,A']\nproxy-groups: [{name: A, type: select}]",
    ] {
        assert!(check(yaml).is_err(), "accepted invalid template: {yaml}");
    }
}

#[test]
fn native_clash_size_is_checked_before_deserialization() {
    let yaml = "x".repeat(deve_sub_domain::MAX_SPEC_BYTES + 1);
    assert!(matches!(
        parse_template_document(&yaml),
        Err(super::error::TemplateAppError::SpecTooLarge(..))
    ));
}

#[test]
fn native_clash_merge_groups_and_escaped_regexes_are_supported() {
    let raw = "proxy-groups:\n  - &base {name: AUTO, type: select, include-all-proxies: true}\n  - {<<: *base, name: PROXY}\nrules: ['DOMAIN-REGEX,\\(,DIRECT', 'MATCH,PROXY']";
    check(raw).expect("merge defaults");
    let native = parse_template_document(raw)
        .expect("parse")
        .spec
        .clash
        .expect("native");
    assert!(!native.contains("<<:"));
    check(include_str!(
        "../../../../tests/fixtures/clash-routing.yaml"
    ))
    .expect("client checked fixture");
    for yaml in [
        "rules: ['IP-CIDR,wrong,DIRECT']",
        "rules: ['IP-CIDR,::1/129,DIRECT']",
        "rules: ['NETWORK,wrong,DIRECT']",
        "apiVersion: deve-sub.io/v1\nkind: SubscriptionTemplate\nmetadata: {name: nested}\nspec: {clash: \"rules: []\"}",
    ] {
        assert!(check(yaml).is_err(), "accepted {yaml}");
    }
}
