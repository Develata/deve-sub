use super::*;

#[test]
fn application_depth_boundary_retains_root_plus_one_convention() {
    let accepted = format!("{}0{}", "[".repeat(9), "]".repeat(9));
    checked_yaml(&accepted).expect("deepest scalar at application depth ten");
    let rejected = format!("{}0{}", "[".repeat(10), "]".repeat(10));
    assert!(matches!(
        checked_yaml(&rejected),
        Err(TemplateAppError::AliasDepthExceeded(11, 10))
    ));
}

#[test]
fn complex_mapping_key_retains_parent_depth_convention() {
    let key = format!("{}leaf{}", "[".repeat(9), "]".repeat(9));
    checked_yaml(&format!("? {key}\n: true\n"))
        .expect("a mapping key does not add an application depth level");
    assert!(matches!(
        checked_yaml(&format!("key: {key}\n")),
        Err(TemplateAppError::AliasDepthExceeded(11, 10))
    ));
}

#[test]
fn v3_scalar_values_preserve_yaml_types_and_integer_boundaries() {
    let yaml = "apiVersion: deve-sub.io/v1\nkind: SubscriptionTemplate\nmetadata: {name: compatibility}\nspec:\n  targetProfiles: [mihomo]\n  variables: {enabled: true, disabled: false, empty: null, feature: on, identifier: '01', max: 18446744073709551615, min: -9223372036854775808, scientific: 1e3}\n";
    let document = parse_template_document(yaml).expect("typed V3 template");
    validate_document(&document, yaml).expect("valid template");
    assert_eq!(
        document.spec.variables,
        serde_json::json!({
            "enabled": true,
            "disabled": false,
            "empty": null,
            "feature": "on",
            "identifier": "01",
            "max": u64::MAX,
            "min": i64::MIN,
            "scientific": 1000.0,
        })
    );
}

#[test]
fn template_parse_errors_do_not_echo_authored_values() {
    let marker = "synthetic-sensitive-marker.invalid";
    for yaml in [
        format!("rules: [\"{marker}\""),
        format!("rules: !custom [{marker}]"),
    ] {
        let error = parse_template_document(&yaml).expect_err("invalid template");
        assert!(matches!(error, TemplateAppError::SpecYamlParse(_)));
        assert!(!error.to_string().contains(marker));
    }
}

#[test]
fn empty_name_reaches_validation_but_missing_scalar_is_a_parse_error() {
    let yaml = "apiVersion: deve-sub.io/v1\nkind: SubscriptionTemplate\nmetadata: {name: ''}\nspec: {targetProfiles: [mihomo]}\n";
    let document = parse_template_document(yaml).expect("an empty name is still a string");
    assert!(matches!(
        validate_document(&document, yaml),
        Err(TemplateAppError::InvalidInput(_))
    ));
    assert!(matches!(
        parse_template_document(&yaml.replace("name: ''", "name:")),
        Err(TemplateAppError::SpecYamlParse(_))
    ));
}
