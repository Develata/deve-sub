//! Normal forward proxy paths with independently authored field expectations.
#![allow(clippy::expect_used)]

use deve_sub_compatibility::{ProfileKind, capability_for, check_node};
use deve_sub_domain::{Authentication, Node, ProtocolConfig, ProtocolKind, SocksDnsResolution};
use deve_sub_protocol::{container::*, parse_uri};
use serde_json::json;

fn fields_equal(a: &Node, b: &Node) {
    assert_eq!(a.display_name, b.display_name);
    assert_eq!(a.protocol, b.protocol);
    assert_eq!(a.endpoint, b.endpoint);
    assert_eq!(a.authentication, b.authentication);
    assert_eq!(a.config, b.config);
    assert_eq!(a.tls, b.tls);
    assert_eq!(a.udp, b.udp);
}

#[test]
fn forward_proxy_uri_verification_aliases_cannot_override_each_other() {
    for query in [
        "skip-cert-verify=1&insecure=0",
        "skip-cert-verify=0&insecure=1",
        "insecure=0&skip-cert-verify=1",
    ] {
        assert!(parse_uri(&format!("https://proxy.example:443?{query}")).is_err());
    }
}

#[test]
fn forward_proxy_security_fields_cannot_silently_override_or_disappear() {
    let entry = json!({"type":"http","name":"ambiguous-sni","server":"proxy.example",
        "port":443,"tls":true,"sni":"first.example","servername":"second.example"});
    let parsed = parse_mihomo_yaml(&json!({"proxies":[entry]}).to_string()).expect("parse");
    assert!(matches!(parsed[0].config, ProtocolConfig::Unsupported(_)));

    let entry = json!({"type":"http","tag":"implicit-utls","server":"proxy.example",
        "server_port":443,"tls":{"enabled":true,"utls":{"enabled":true}}});
    let parsed = parse_singbox_json(&json!({"outbounds":[entry]}).to_string()).expect("parse");
    assert!(matches!(parsed[0].config, ProtocolConfig::Unsupported(_)));
}

#[test]
fn clash_forward_proxies_retain_fields_through_every_native_target_and_uri() {
    for host in ["192.0.2.20", "2001:db8::20", "proxy.example"] {
        for kind in ["socks5", "http", "https"] {
            for credentials in [false, true] {
                let mut entry =
                    json!({"name":"测试 proxy @ # /","type":kind,"server":host,"port":8443});
                if credentials {
                    entry["username"] = json!("test+user@示例");
                    entry["password"] = json!("TEST:p@ss/?#%&+");
                }
                let nodes = parse_mihomo_yaml(&json!({"proxies":[entry]}).to_string())
                    .expect("Clash input");
                let node = &nodes[0];
                assert_eq!(node.display_name, "测试 proxy @ # /");
                assert_eq!(node.endpoint.port, 8443);
                assert_eq!(
                    node.protocol,
                    if kind == "socks5" {
                        ProtocolKind::Socks5
                    } else {
                        ProtocolKind::Http
                    }
                );
                assert_eq!(
                    node.tls.as_ref().is_some_and(|tls| tls.enabled),
                    kind == "https"
                );
                if credentials {
                    assert_eq!(
                        node.authentication,
                        Authentication::UserPassword {
                            username: "test+user@示例".into(),
                            password: "TEST:p@ss/?#%&+".into(),
                        }
                    );
                } else {
                    assert_eq!(node.authentication, Authentication::None);
                }
                let mihomo = deve_sub_emitter::emit_mihomo(&nodes).expect("Mihomo output");
                fields_equal(
                    node,
                    &parse_mihomo_yaml(&mihomo).expect("Mihomo reparse")[0],
                );
                let singbox = deve_sub_emitter::emit_singbox(&nodes).expect("sing-box output");
                fields_equal(
                    node,
                    &parse_singbox_json(&singbox).expect("sing-box reparse")[0],
                );
                let xray = deve_sub_emitter::emit_xray(&nodes).expect("Xray output");
                fields_equal(node, &parse_xray_json(&xray).expect("Xray reparse")[0]);
                let v2ray = deve_sub_emitter::emit_v2ray(&nodes).expect("V2Ray output");
                fields_equal(node, &parse_v2ray_json(&v2ray).expect("V2Ray reparse")[0]);
                let uri = deve_sub_emitter::emit_uri(node).expect("URI output");
                fields_equal(node, &parse_uri(&uri).expect("URI reparse"));
                let canonical = deve_sub_emitter::emit_json(&nodes).expect("Canonical JSON");
                assert_eq!(
                    serde_json::from_str::<Vec<Node>>(&canonical).expect("Canonical parse"),
                    nodes
                );
                for target in [
                    ProfileKind::Mihomo,
                    ProfileKind::SingBox,
                    ProfileKind::Xray,
                    ProfileKind::V2Ray,
                    ProfileKind::UriList,
                    ProfileKind::Json,
                ] {
                    assert!(
                        check_node(node, &capability_for(target)).is_ok(),
                        "{target:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn socks_uri_dns_intent_is_typed_and_never_silently_downgraded() {
    for (scheme, policy) in [
        ("socks", SocksDnsResolution::Unspecified),
        ("socks5", SocksDnsResolution::Local),
        ("socks5h", SocksDnsResolution::Remote),
    ] {
        let node = parse_uri(&format!(
            "{scheme}://test%40user:TEST%3Apassword@[2001:db8::1]:1080#DNS"
        ))
        .expect("URI");
        let ProtocolConfig::Socks5(config) = &node.config else {
            panic!("typed SOCKS5 required")
        };
        assert_eq!(config.dns_resolution, policy);
        fields_equal(
            &node,
            &parse_uri(&deve_sub_emitter::emit_uri(&node).expect("emit URI")).expect("reparse"),
        );
        if policy != SocksDnsResolution::Unspecified {
            for target in [
                ProfileKind::Mihomo,
                ProfileKind::SingBox,
                ProfileKind::Xray,
                ProfileKind::V2Ray,
                ProfileKind::Shadowrocket,
            ] {
                assert!(
                    check_node(&node, &capability_for(target)).is_err(),
                    "{target:?}"
                );
            }
            assert!(deve_sub_emitter::emit_mihomo(std::slice::from_ref(&node)).is_err());
            assert!(deve_sub_emitter::emit_singbox(std::slice::from_ref(&node)).is_err());
        }
    }
}

#[test]
fn https_verification_three_states_and_sni_survive_native_round_trips() {
    for verify in [None, Some(false), Some(true)] {
        let mut entry = json!({"name":"HTTPS","type":"http","server":"proxy.example","port":443,"tls":true,"sni":"cert.example"});
        if let Some(verify) = verify {
            entry["skip-cert-verify"] = json!(verify);
        }
        let nodes = parse_mihomo_yaml(&json!({"proxies":[entry]}).to_string()).expect("Clash");
        assert_eq!(nodes[0].tls.as_ref().expect("TLS").skip_cert_verify, verify);
        for output in [
            deve_sub_emitter::emit_singbox(&nodes).expect("sing-box"),
            deve_sub_emitter::emit_xray(&nodes).expect("Xray"),
        ] {
            fields_equal(&nodes[0], &parse_auto_json(&output).expect("reparse")[0]);
        }
        fields_equal(
            &nodes[0],
            &parse_uri(&deve_sub_emitter::emit_uri(&nodes[0]).expect("URI")).expect("reparse"),
        );
    }
}

#[test]
fn http_headers_are_typed_and_incompatible_targets_reject() {
    let nodes = parse_singbox_json(
        &json!({"outbounds":[{
            "type":"http","tag":"header-proxy","server":"proxy.example","server_port":8080,
            "headers":{"X-Test":["one", "two"]},"path":"/connect"
        }]})
        .to_string(),
    )
    .expect("parse");
    assert!(matches!(nodes[0].config, ProtocolConfig::Http(_)));
    fields_equal(
        &nodes[0],
        &parse_singbox_json(&deve_sub_emitter::emit_singbox(&nodes).expect("emit"))
            .expect("reparse")[0],
    );
    assert!(deve_sub_emitter::emit_uri(&nodes[0]).is_err());
    assert!(deve_sub_emitter::emit_mihomo(&nodes).is_err());
    assert!(check_node(&nodes[0], &capability_for(ProfileKind::Mihomo)).is_err());
}

#[test]
fn unsupported_container_entry_keeps_name_endpoint_and_original_data() {
    let entry = json!({"type":"socks5","name":"inspect-me","server":"192.0.2.19","port":1080,"unmapped-security":true});
    let nodes = parse_mihomo_yaml(&json!({"proxies":[entry]}).to_string()).expect("parse");
    assert_eq!(nodes[0].display_name, "inspect-me");
    assert_eq!(nodes[0].endpoint.host.uri_host(), "192.0.2.19");
    assert_eq!(nodes[0].endpoint.port, 1080);
    assert_eq!(nodes[0].protocol, ProtocolKind::Socks5);
    let ProtocolConfig::Unsupported(raw) = &nodes[0].config else {
        panic!("unmapped fields must be retained")
    };
    assert_eq!(raw.raw, entry);
}

#[test]
fn explicit_disabled_tls_and_udp_policies_are_preserved_or_rejected() {
    for kind in ["socks5", "http"] {
        let nodes = parse_mihomo_yaml(
            &json!({"proxies":[{"name":"plain","type":kind,"server":"proxy.example",
                "port":8080,"tls":false}]})
            .to_string(),
        )
        .expect("Clash");
        let node = &nodes[0];
        assert!(!node.tls.as_ref().expect("explicit TLS state").enabled);
        for output in [
            deve_sub_emitter::emit_mihomo(&nodes).expect("Mihomo"),
            deve_sub_emitter::emit_xray(&nodes).expect("Xray"),
            deve_sub_emitter::emit_v2ray(&nodes).expect("V2Ray"),
        ] {
            let parsed = if output.trim_start().starts_with('{') {
                parse_auto_json(&output).expect("JSON")
            } else {
                parse_mihomo_yaml(&output).expect("YAML")
            };
            fields_equal(node, &parsed[0]);
        }
        fields_equal(
            node,
            &parse_uri(&deve_sub_emitter::emit_uri(node).expect("URI")).expect("URI parse"),
        );
    }
    for udp in [false, true] {
        let nodes = parse_mihomo_yaml(
            &json!({"proxies":[{"name":"udp","type":"socks5","server":"proxy.example",
                "port":1080,"udp":udp}]})
            .to_string(),
        )
        .expect("Clash");
        let output = deve_sub_emitter::emit_singbox(&nodes).expect("sing-box");
        fields_equal(&nodes[0], &parse_singbox_json(&output).expect("parse")[0]);
        assert!(deve_sub_emitter::emit_xray(&nodes).is_err());
        assert!(check_node(&nodes[0], &capability_for(ProfileKind::Xray)).is_err());
    }
    let node = parse_uri("http://proxy.example:80?udp=0#HTTP").expect("HTTP");
    assert_eq!(node.udp.supported, Some(false));
    assert!(deve_sub_emitter::emit_singbox(std::slice::from_ref(&node)).is_err());
    assert!(deve_sub_emitter::emit_xray(std::slice::from_ref(&node)).is_err());
    assert!(check_node(&node, &capability_for(ProfileKind::SingBox)).is_err());
}

#[test]
fn xray_http_headers_have_the_official_settings_location() {
    let input = json!({"outbounds":[{"tag":"headers","protocol":"http",
        "settings":{"servers":[{"address":"proxy.example","port":8080,
            "users":[{"user":"test-user","pass":"TEST_PASSWORD"}]}],
            "headers":{"X-Fixture":"one"}}}]});
    let nodes = parse_xray_json(&input.to_string()).expect("Xray");
    let emitted = deve_sub_emitter::emit_xray(&nodes).expect("emit");
    let output: serde_json::Value = serde_json::from_str(&emitted).expect("JSON");
    assert_eq!(
        output["outbounds"][0]["settings"]["headers"]["X-Fixture"],
        "one"
    );
    assert!(output["outbounds"][0]["settings"]["servers"][0]["headers"].is_null());
    fields_equal(&nodes[0], &parse_xray_json(&emitted).expect("reparse")[0]);
    assert!(deve_sub_emitter::emit_v2ray(&nodes).is_err());
    assert!(check_node(&nodes[0], &capability_for(ProfileKind::V2Ray)).is_err());
}

#[test]
fn disabled_tls_with_security_fields_never_loses_them_during_emission() {
    let mut node = parse_uri("http://proxy.example:80?tls=0#plain").expect("URI");
    node.tls.as_mut().expect("TLS state").skip_cert_verify = Some(false);
    for target in [
        ProfileKind::Mihomo,
        ProfileKind::SingBox,
        ProfileKind::Xray,
        ProfileKind::V2Ray,
        ProfileKind::UriList,
    ] {
        assert!(
            check_node(&node, &capability_for(target)).is_err(),
            "{target:?}"
        );
    }
    assert!(deve_sub_emitter::emit_uri(&node).is_err());
    assert!(deve_sub_emitter::emit_xray(std::slice::from_ref(&node)).is_err());
}
