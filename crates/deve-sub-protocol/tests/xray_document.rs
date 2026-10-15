//! PARSE-007/008 and SEC-005: owned Xray-family results preserve complete input
//! identity, document-error priority and raw entry limits.

#![allow(clippy::expect_used)]

use deve_sub_domain::{Authentication, Node, ProtocolConfig, ProtocolKind};
use deve_sub_protocol::{
    ParseError,
    container::{MAX_ENTRIES, parse_auto_json, parse_v2ray_json, parse_xray_json},
};
use serde_json::{Value, json};

type Parser = fn(&str) -> Result<Vec<Node>, ParseError>;
const PARSERS: [(&str, Parser); 3] = [
    ("xray-json", parse_auto_json),
    ("xray-json", parse_xray_json),
    ("v2ray-json", parse_v2ray_json),
];

fn assert_unknown(node: &Node, raw: &Value, format: &str, reason: &str) {
    let ProtocolConfig::Unsupported(unknown) = &node.config else {
        panic!("unsupported entry must remain inspectable");
    };
    assert_eq!(unknown.raw, *raw);
    assert_eq!(unknown.raw_format.as_deref(), Some(format));
    assert_eq!(unknown.reason, reason);
}

#[test]
fn mixed_outbounds_preserve_order_owned_fields_and_unsupported_identity() {
    let entries = json!([
        {
            "protocol": "trojan", "tag": "repeated-tag",
            "settings": {"servers": [{
                "address": "before.example.com", "port": 443, "password": "TEST_BEFORE"
            }]},
            "streamSettings": {"security": "tls", "tlsSettings": {
                "serverName": "sni.example.com", "allowInsecure": false
            }}
        },
        {"protocol": "future", "tag": "repeated-tag", "extra": {
            "nested": [true, null, {"integer": u64::MAX}]
        }},
        null,
        {"protocol": "trojan", "settings": {"servers": []}, "extra": [1, 2]},
        {
            "protocol": "vless", "tag": "non-reality",
            "settings": {"vnext": [{
                "address": "vless.example.com", "port": 8443,
                "users": [{"id": "00000000-0000-4000-8000-000000000001"}]
            }]},
            "streamSettings": {"security": "tls", "tlsSettings": {"allowInsecure": true}}
        },
        {
            "protocol": "trojan", "tag": "repeated-tag",
            "settings": {"servers": [{
                "address": "after.example.com", "port": 9443, "password": "TEST_AFTER"
            }]}
        }
    ]);
    for (format, parse) in PARSERS {
        let body = json!({"outbounds": entries, "ignored_root": {"type": "unused"}}).to_string();
        let nodes = parse(&body).expect("mixed input");
        drop(body);
        assert_eq!(nodes.len(), 6);
        assert!(nodes.iter().all(|node| node.source.raw_uri.is_none()));
        for (index, host, port, password) in [
            (0, "before.example.com", 443, "TEST_BEFORE"),
            (5, "after.example.com", 9443, "TEST_AFTER"),
        ] {
            let node = &nodes[index];
            assert_eq!(node.protocol, ProtocolKind::Trojan);
            assert!(matches!(node.config, ProtocolConfig::Trojan(_)));
            assert_eq!(node.display_name, "repeated-tag");
            assert_eq!(node.endpoint.host.uri_host(), host);
            assert_eq!(node.endpoint.port, port);
            let Authentication::Password { password: actual } = &node.authentication else {
                panic!("Trojan password");
            };
            assert_eq!(actual, password);
        }
        let tls = nodes[0].tls.as_ref().expect("explicit TLS");
        assert!(tls.enabled);
        assert_eq!(tls.server_name.as_deref(), Some("sni.example.com"));
        assert_eq!(tls.skip_cert_verify, Some(false));
        assert_eq!(nodes[1].protocol, ProtocolKind::Unknown("future".into()));
        assert_unknown(
            &nodes[1],
            &entries[1],
            format,
            "unsupported proxy type: future",
        );
        assert_eq!(nodes[2].protocol, ProtocolKind::Unknown(String::new()));
        assert_unknown(&nodes[2], &entries[2], format, "missing 'protocol' field");
        assert_eq!(nodes[3].protocol, ProtocolKind::Unknown("trojan".into()));
        assert_unknown(
            &nodes[3],
            &entries[3],
            format,
            "missing required field: servers[0]",
        );
        // WHY: VLESS without Reality retains its typed identity and mapped
        // endpoint/authentication even though its full raw entry is Unsupported.
        let vless = &nodes[4];
        assert_eq!(vless.protocol, ProtocolKind::Vless);
        assert_eq!(vless.display_name, "non-reality");
        assert_eq!(vless.endpoint.host.uri_host(), "vless.example.com");
        assert_eq!(vless.endpoint.port, 8443);
        let Authentication::Uuid { uuid } = &vless.authentication else {
            panic!("VLESS UUID");
        };
        assert_eq!(uuid, "00000000-0000-4000-8000-000000000001");
        assert_eq!(
            vless.tls.as_ref().expect("VLESS TLS").skip_cert_verify,
            Some(true)
        );
        assert_unknown(
            vless,
            &entries[4],
            format,
            "VLESS without Reality is not P0",
        );
    }
}

#[test]
fn explicit_family_preserves_shape_errors_and_validates_ignored_content() {
    for parse in [parse_xray_json as Parser, parse_v2ray_json as Parser] {
        for (body, expected) in [
            ("null", "outbounds"),
            ("[]", "outbounds"),
            ("{}", "outbounds"),
            (r#"{"outbounds":null}"#, "outbounds (not a list)"),
            (r#"{"outbounds":{}}"#, "outbounds (not a list)"),
        ] {
            assert!(
                matches!(parse(body), Err(ParseError::MissingContainerKey(key)) if key == expected)
            );
        }
        assert!(parse(r#"{"outbounds":[]}"#).expect("empty").is_empty());
        for body in [
            r#"{"outbounds":[]} trailing"#.to_owned(),
            r#"{"outbounds":null,"ignored":[}"#.to_owned(),
            format!(
                "{{\"outbounds\":[],\"ignored\":{}0{}}}",
                "[".repeat(128),
                "]".repeat(128)
            ),
        ] {
            assert!(matches!(parse(&body), Err(ParseError::InvalidJson(_))));
        }
    }
}

#[test]
fn xray_family_counts_every_raw_entry_after_complete_json_validation() {
    let entry = json!({"protocol": "future"});
    let at_limit = json!({"outbounds": vec![entry.clone(); MAX_ENTRIES]}).to_string();
    let over_limit = json!({"outbounds": vec![entry.clone(); MAX_ENTRIES + 1]}).to_string();
    for (format, parse) in PARSERS {
        let nodes = parse(&at_limit).expect("exact entry limit");
        assert_eq!(nodes.len(), MAX_ENTRIES);
        for node in &nodes {
            assert_unknown(node, &entry, format, "unsupported proxy type: future");
        }
        assert!(
            matches!(parse(&over_limit), Err(ParseError::TooManyEntries(count)) if count == MAX_ENTRIES + 1)
        );
        // WHY: even an over-limit array must not hide malformed trailing input.
        assert!(matches!(
            parse(&(over_limit.clone() + " trailing")),
            Err(ParseError::InvalidJson(_))
        ));
    }
}
