//! PARSE-006/007/008/026 and SEC-005: automatic JSON dispatch preserves the
//! frozen document errors, raw format identity and two-pass ShadowTLS mapping.

#![allow(clippy::expect_used)]

use deve_sub_domain::{Authentication, Node, ProtocolConfig, ProtocolKind, ShadowTlsVersion};
use deve_sub_protocol::{
    ParseError,
    container::{MAX_ENTRIES, parse_auto_json, parse_v2ray_json, parse_xray_json},
};

const SHARED_SHADOWTLS: &str = include_str!("fixtures/auto-json-shadow-shared.json");

#[test]
fn shape_errors_keep_the_existing_singbox_identity() {
    for (input, expected) in [
        ("{}", "outbounds"),
        ("[]", "outbounds"),
        ("null", "outbounds"),
        (r#"{"outbounds":null}"#, "outbounds (not a list)"),
        (r#"{"outbounds":{}}"#, "outbounds (not a list)"),
    ] {
        let error = parse_auto_json(input).expect_err("invalid shape");
        assert!(matches!(error, ParseError::MissingContainerKey(key) if key == expected));
        assert_eq!(
            error.to_string(),
            format!("missing container key: {expected}")
        );
    }
    assert!(
        parse_auto_json(r#"{"outbounds":[]}"#)
            .expect("empty list")
            .is_empty()
    );
}

#[test]
fn any_type_value_wins_over_protocol_keys() {
    for marker in serde_json::json!([null, 3, false, [], {}])
        .as_array()
        .expect("markers")
    {
        let outbound = serde_json::json!({ "type": marker, "protocol": "future" });
        let body = serde_json::json!({ "outbounds": [outbound] }).to_string();
        let nodes = parse_auto_json(&body).expect("preserved unknown entry");
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].source.raw_uri.is_none());
        let ProtocolConfig::Unsupported(unknown) = &nodes[0].config else {
            panic!("a non-string type remains unsupported sing-box input");
        };
        assert_eq!(unknown.raw_format.as_deref(), Some("singbox-json"));
        assert_eq!(unknown.reason, "missing 'type' field");
        assert_eq!(unknown.raw, outbound);
    }
}

#[test]
fn duplicate_keys_keep_last_value_before_dialect_detection() {
    for (body, format, raw) in [
        (
            r#"{"outbounds":[{"protocol":"future"}],"outbounds":[{"type":"future"}]}"#,
            "singbox-json",
            serde_json::json!({ "type": "future" }),
        ),
        (
            r#"{"outbounds":[{"type":"future"}],"outbounds":[{"protocol":"future"}]}"#,
            "xray-json",
            serde_json::json!({ "protocol": "future" }),
        ),
        (
            r#"{"outbounds":[{"type":"trojan","type":null,"protocol":"future"}]}"#,
            "singbox-json",
            serde_json::json!({ "type": null, "protocol": "future" }),
        ),
    ] {
        let nodes = parse_auto_json(body).expect("duplicate JSON keys");
        assert_eq!(nodes.len(), 1);
        let ProtocolConfig::Unsupported(unknown) = &nodes[0].config else {
            panic!("unknown fixture remains preserved");
        };
        assert_eq!(unknown.raw_format.as_deref(), Some(format));
        assert_eq!(unknown.raw, raw);
    }
}

#[test]
fn auto_xray_and_explicit_v2ray_retain_their_frozen_format_identity() {
    type Parser = fn(&str) -> Result<Vec<Node>, ParseError>;
    let raw = serde_json::json!({ "protocol": "future", "extra": [null, true, u64::MAX] });
    let body = serde_json::json!({ "outbounds": [raw] }).to_string();
    for (parse, format) in [
        (parse_auto_json as Parser, "xray-json"),
        (parse_xray_json as Parser, "xray-json"),
        (parse_v2ray_json as Parser, "v2ray-json"),
    ] {
        let nodes = parse(&body).expect("unknown Xray-family entry");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].protocol, ProtocolKind::Unknown("future".into()));
        assert!(nodes[0].source.raw_uri.is_none());
        let ProtocolConfig::Unsupported(unknown) = &nodes[0].config else {
            panic!("unknown fixture remains preserved");
        };
        assert_eq!(unknown.raw_format.as_deref(), Some(format));
        assert_eq!(unknown.reason, "unsupported proxy type: future");
        assert_eq!(unknown.raw, raw);
        assert_eq!(unknown.raw["extra"][2].as_u64(), Some(u64::MAX));
    }
}

#[test]
fn shared_shadowtls_wrapper_before_or_after_inners_keeps_order_credentials_and_tls() {
    let mut document: serde_json::Value = serde_json::from_str(SHARED_SHADOWTLS).expect("fixture");
    for wrapper_first in [false, true] {
        if wrapper_first {
            document["outbounds"]
                .as_array_mut()
                .expect("outbounds")
                .rotate_right(1);
        }
        let nodes = parse_auto_json(&document.to_string()).expect("shared wrapper");
        assert_eq!(nodes.len(), 2);
        // WHY: these expected fields come from the frozen pre-change parser
        // oracle, not another entry point sharing the candidate mapper.
        for (node, (name, password)) in nodes
            .iter()
            .zip([("inner", "TEST_INNER"), ("inner2", "TEST_INNER2")])
        {
            assert_eq!(node.display_name, name);
            assert_eq!(node.protocol, ProtocolKind::ShadowTls);
            assert_eq!(node.endpoint.host.uri_host(), "wrapper.example.test");
            assert_eq!(node.endpoint.port, 8443);
            assert_eq!(
                node.authentication,
                Authentication::Password {
                    password: password.into()
                }
            );
            assert!(node.source.raw_uri.is_none());
            let ProtocolConfig::ShadowTls(config) = &node.config else {
                panic!("wrapper and inner must merge");
            };
            assert_eq!(config.version, ShadowTlsVersion::V3);
            assert_eq!(config.password.as_deref(), Some("TEST_WRAPPER"));
            assert_eq!(config.inner_protocol, ProtocolKind::Trojan);
            assert!(matches!(
                config.inner_config.as_ref(),
                ProtocolConfig::Trojan(_)
            ));
            let tls = node.tls.as_ref().expect("wrapper TLS");
            assert!(tls.enabled);
            assert_eq!(tls.server_name.as_deref(), Some("cover.example.test"));
            assert_eq!(tls.skip_cert_verify, None);
        }
    }
}

#[test]
fn raw_outbound_count_is_checked_before_shadowtls_absorption() {
    let document: serde_json::Value = serde_json::from_str(SHARED_SHADOWTLS).expect("fixture");
    let mut outbounds = vec![document["outbounds"][0].clone(); MAX_ENTRIES];
    outbounds.push(document["outbounds"][2].clone());
    let body = serde_json::json!({ "outbounds": outbounds }).to_string();
    assert!(matches!(
        parse_auto_json(&body),
        Err(ParseError::TooManyEntries(10_001))
    ));
}

#[test]
fn complete_document_errors_precede_entry_count_and_mapping() {
    for body in [r#"{"outbounds":[]} {}"#, r#"{"outbounds":[]} trailing"#] {
        let ParseError::InvalidJson(message) = parse_auto_json(body).expect_err("trailing data")
        else {
            panic!("the entire JSON document must be parsed first");
        };
        assert_eq!(message, "trailing characters at line 1 column 18");
    }
    let entries = vec![r#"{"protocol":"future"}"#; MAX_ENTRIES + 1].join(",");
    let body = format!("{{\"outbounds\":[{entries}]}} bad");
    let ParseError::InvalidJson(message) =
        parse_auto_json(&body).expect_err("over-count with tail")
    else {
        panic!("invalid trailing input takes precedence over the raw entry limit");
    };
    assert_eq!(message, "trailing characters at line 1 column 220039");
    let body = format!(
        "{{\"outbounds\":[{entries}],\"extra\":{}0{}}}",
        "[".repeat(200),
        "]".repeat(200)
    );
    assert!(
        matches!(parse_auto_json(&body), Err(ParseError::InvalidJson(message)) if message.starts_with("recursion limit exceeded"))
    );
}
