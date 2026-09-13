use super::*;

#[test]
fn validate_output_rejects_empty() {
    assert!(validate_output("", ProfileKind::Mihomo).is_err());
    assert!(validate_output("   \n  ", ProfileKind::SingBox).is_err());
}

#[test]
fn validate_output_accepts_valid_yaml() {
    let yaml = "proxies:\n  - name: test\n    type: trojan\n    server: a.com\n    port: 443\n";
    assert!(validate_output(yaml, ProfileKind::Mihomo).is_ok());
}

#[test]
fn validate_output_accepts_valid_json() {
    let json = r#"{"outbounds":[]}"#;
    assert!(validate_output(json, ProfileKind::SingBox).is_ok());
    assert!(validate_output(json, ProfileKind::Xray).is_ok());
    assert!(validate_output(json, ProfileKind::V2Ray).is_ok());
}

#[test]
fn validate_output_rejects_invalid_json() {
    assert!(validate_output("{not json", ProfileKind::SingBox).is_err());
}

#[test]
fn validate_output_skips_parse_for_uri_list() {
    assert!(validate_output("trojan://pw@host:443", ProfileKind::UriList).is_ok());
    assert!(validate_output("any non-empty text", ProfileKind::Shadowrocket).is_ok());
}

#[test]
fn validate_output_mihomo_missing_proxies_rejected() {
    let yaml = "proxy-groups: []\nrules: []\n";
    let err = validate_output(yaml, ProfileKind::Mihomo)
        .expect_err("mihomo without proxies must fail structural validation");
    assert!(
        matches!(err, TemplateAppError::InvalidStructure(ref m) if m.contains("proxies")),
        "got {err:?}"
    );
}

#[test]
fn validate_output_mihomo_empty_proxies_array_rejected() {
    let yaml = "proxies: []\n";
    let err =
        validate_output(yaml, ProfileKind::Mihomo).expect_err("empty proxies array must fail");
    assert!(
        matches!(err, TemplateAppError::InvalidStructure(ref m) if m.contains("empty")),
        "got {err:?}"
    );
}

#[test]
fn validate_output_mihomo_proxies_not_array_rejected() {
    let yaml = "proxies: notarray\n";
    let err = validate_output(yaml, ProfileKind::Mihomo).expect_err("non-array proxies must fail");
    assert!(
        matches!(err, TemplateAppError::InvalidStructure(ref m) if m.contains("not an array")),
        "got {err:?}"
    );
}

#[test]
fn validate_output_json_scalar_rejected() {
    let err = validate_output("\"just a string\"", ProfileKind::SingBox)
        .expect_err("JSON scalar must fail structural validation");
    assert!(
        matches!(err, TemplateAppError::InvalidStructure(ref m) if m.contains("array or object")),
        "got {err:?}"
    );
    let err = validate_output("42", ProfileKind::Xray).expect_err("JSON number scalar must fail");
    assert!(
        matches!(err, TemplateAppError::InvalidStructure(_)),
        "got {err:?}"
    );
}

#[test]
fn validate_output_json_array_accepted() {
    assert!(validate_output("[]", ProfileKind::SingBox).is_ok());
    assert!(validate_output(r#"{"outbounds":[]}"#, ProfileKind::V2Ray).is_ok());
}

#[test]
fn validate_output_uri_list_only_blank_lines_rejected() {
    let err = validate_output("   \n\n  \n", ProfileKind::UriList)
        .expect_err("blank-only uri_list must fail");
    assert!(
        matches!(err, TemplateAppError::EmptyOutput),
        "blank-only uri_list should be EmptyOutput, got {err:?}"
    );
    let err = validate_output("   \n\n  \n", ProfileKind::Shadowrocket)
        .expect_err("blank-only shadowrocket must fail");
    assert!(matches!(err, TemplateAppError::EmptyOutput), "got {err:?}");
}

#[test]
fn validate_output_uri_list_with_content_accepted() {
    assert!(validate_output("trojan://a@b:1\ntrojan://c@d:2\n", ProfileKind::UriList).is_ok());
}

#[test]
fn emit_dispatches_all_profiles() {
    let template = AssembledTemplate::from_nodes(vec![]);
    assert!(emit(ProfileKind::Mihomo, &template).is_ok());
    assert!(emit(ProfileKind::SingBox, &template).is_ok());
    assert!(emit(ProfileKind::Xray, &template).is_ok());
    assert!(emit(ProfileKind::V2Ray, &template).is_ok());
    assert!(emit(ProfileKind::UriList, &template).is_ok());
}
