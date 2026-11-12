#![allow(clippy::expect_used)]
//! PARSE-026: explicit wrappers cannot become an ordinary proxy on failure.
use deve_sub_compatibility::{ProfileKind, capability_for, check_node};
use deve_sub_domain::{ProtocolConfig, ProtocolKind};
use deve_sub_emitter::{emit_mihomo, emit_singbox};
use deve_sub_protocol::container::parse_mihomo_yaml;
use serde_json::{Value, json};

fn parse(entry: Value) -> deve_sub_domain::Node {
    parse_mihomo_yaml(&json!({"proxies": [entry]}).to_string())
        .expect("source parse")
        .remove(0)
}

fn ss() -> Value {
    json!({"type":"ss", "name":"reserved-SS", "server":"ss.example.test", "port":443,
        "cipher":"aes-128-gcm", "password":"TEST_INNER_PASSWORD", "plugin":"shadow-tls",
        "plugin-opts":{"version":3, "password":"TEST_WRAPPER_PASSWORD", "host":"cover.example.test"}})
}

#[test]
fn malformed_explicit_wrapper_never_degrades_to_inner_protocol() {
    for opts in [
        Value::Null,
        json!([]),
        json!({}),
        json!({"version":null}),
        json!({"version":4}),
        json!({"version":-1}),
        json!({"version":"3"}),
        json!({"version":4294967299_u64}),
        json!({"version":3}),
        json!({"version":2,"password":""}),
        json!({"version":3,"password":true}),
        json!({"version":3,"password":[]}),
        json!({"version":3,"password":"TEST_WRAPPER_PASSWORD","unknown-security":true}),
    ] {
        let mut entry = ss();
        entry["plugin-opts"] = opts;
        let node = parse(entry.clone());
        let ProtocolConfig::Unsupported(raw) = &node.config else {
            panic!("invalid wrapper accepted");
        };
        assert_eq!(raw.raw, entry, "original input retained");
        assert!(!raw.reason.contains("TEST_WRAPPER_PASSWORD"));
        for profile in [ProfileKind::Mihomo, ProfileKind::SingBox, ProfileKind::Xray] {
            assert!(check_node(&node, &capability_for(profile)).is_err());
        }
        assert!(emit_mihomo(&[node]).is_err());
    }
    let plain = parse(
        json!({"type":"ss", "server":"ss.example.test", "port":443,"cipher":"aes-128-gcm","password":"TEST_INNER_PASSWORD"}),
    );
    assert_eq!(plain.protocol, ProtocolKind::Shadowsocks);
}

#[test]
fn conflicting_and_foreign_markers_are_retained_as_unsupported() {
    for marker in ["restls-opts", "jls-opts"] {
        let mut conflict = ss();
        conflict[marker] = json!({});
        assert!(matches!(
            parse(conflict).config,
            ProtocolConfig::Unsupported(_)
        ));
    }
    let mut conflict = ss();
    conflict["shadow-tls-opts"] = json!({"version":3,"password":"TEST_WRAPPER_PASSWORD"});
    assert!(matches!(
        parse(conflict).config,
        ProtocolConfig::Unsupported(_)
    ));
    for plugin in ["obfs", "v2ray-plugin"] {
        let mut conflict = ss();
        conflict["plugin"] = json!(plugin);
        conflict["plugin-opts"] = if plugin == "obfs" {
            json!({"mode":"http"})
        } else {
            json!({"mode":"websocket"})
        };
        conflict["shadow-tls-opts"] = json!({"version":3,"password":"TEST_WRAPPER_PASSWORD"});
        assert!(matches!(
            parse(conflict).config,
            ProtocolConfig::Unsupported(_)
        ));
    }
    let foreign = json!({"type":"trojan","server":"trojan.example.test","port":443,"password":"TEST_INNER_PASSWORD","plugin":"shadow-tls","plugin-opts":{"version":3,"password":"TEST_WRAPPER_PASSWORD"}});
    assert!(matches!(
        parse(foreign).config,
        ProtocolConfig::Unsupported(_)
    ));
}

#[test]
fn valid_ss_wrapper_has_one_plugin_and_strict_yaml_round_trip() {
    for version in [1, 2, 3] {
        let mut entry = ss();
        entry["plugin-opts"]["version"] = json!(version);
        if version == 1 {
            entry["plugin-opts"]
                .as_object_mut()
                .expect("map")
                .remove("password");
        }
        let original = parse(entry);
        check_node(&original, &capability_for(ProfileKind::Mihomo)).expect("supported wrapper");
        let output = emit_mihomo(std::slice::from_ref(&original)).expect("emit");
        deve_sub_yaml::parse_generated_value(&output).expect("strict generated YAML");
        assert_eq!(output.matches("    plugin:").count(), 1);
        assert_eq!(output.matches("    plugin-opts:").count(), 1);
        let replay = parse_mihomo_yaml(&output).expect("reparse").remove(0);
        assert_eq!(original.config, replay.config);
        assert_eq!(original.authentication, replay.authentication);
        assert_eq!(original.tls, replay.tls);
    }
}

#[test]
fn canonical_invalid_password_and_plugin_conflicts_are_rejected_before_emit() {
    for password in [None, Some(String::new())] {
        let mut node = parse(ss());
        let ProtocolConfig::ShadowTls(cfg) = &mut node.config else {
            panic!("wrapper");
        };
        cfg.password = password;
        for profile in [ProfileKind::Mihomo, ProfileKind::SingBox] {
            assert!(check_node(&node, &capability_for(profile)).is_err());
        }
        assert!(emit_mihomo(std::slice::from_ref(&node)).is_err());
        assert!(emit_singbox(&[node]).is_err());
    }
    let mut node = parse(ss());
    let ProtocolConfig::ShadowTls(cfg) = &mut node.config else {
        panic!("wrapper");
    };
    let ProtocolConfig::Shadowsocks(inner) = cfg.inner_config.as_mut() else {
        panic!("inner");
    };
    inner.plugin = Some("obfs-local".into());
    inner.plugin_opts = Some("obfs=http".into());
    assert!(check_node(&node, &capability_for(ProfileKind::Mihomo)).is_err());
    assert!(emit_mihomo(&[node]).is_err());
}

#[test]
fn unmapped_tls_policy_is_rejected_instead_of_discarded() {
    for (key, value) in [
        ("skip-cert-verify", json!(false)),
        ("alpn", json!(["h2"])),
        ("client-fingerprint", json!("chrome")),
    ] {
        let mut entry = ss();
        entry[key] = value;
        let node = parse(entry);
        assert!(check_node(&node, &capability_for(ProfileKind::Mihomo)).is_err());
        assert!(emit_mihomo(&[node]).is_err());
    }
    for (key, value) in [
        ("skip-cert-verify", json!("false")),
        ("alpn", json!("h2")),
        ("client-fingerprint", json!(true)),
        ("sni", json!(false)),
    ] {
        let mut entry = ss();
        entry[key] = value;
        assert!(matches!(
            parse(entry).config,
            ProtocolConfig::Unsupported(_)
        ));
    }
}

#[test]
fn snell_wrapper_alias_round_trips_and_independent_obfuscation_is_rejected() {
    let entry = json!({"type":"snell","name":"reserved-Snell","server":"snell.example.test","port":443,"psk":"TEST_INNER_PASSWORD","version":4,"obfs-opts":{"mode":"shadow-tls","version":3,"password":"TEST_WRAPPER_PASSWORD","host":"cover.example.test"}});
    let node = parse(entry.clone());
    check_node(&node, &capability_for(ProfileKind::Mihomo)).expect("Snell wrapper");
    let output = emit_mihomo(std::slice::from_ref(&node)).expect("emit Snell");
    deve_sub_yaml::parse_generated_value(&output).expect("strict YAML");
    let replay = parse_mihomo_yaml(&output).expect("parse").remove(0);
    assert_eq!(node.config, replay.config);
    assert_eq!(node.tls, replay.tls);
    let mut fallback = entry.clone();
    fallback["obfs-opts"]
        .as_object_mut()
        .expect("opts")
        .remove("host");
    fallback["sni"] = json!("cover.example.test");
    let fallback = parse(fallback);
    check_node(&fallback, &capability_for(ProfileKind::Mihomo)).expect("host fallback");
    let output = emit_mihomo(std::slice::from_ref(&fallback)).expect("fallback emit");
    let replay = parse_mihomo_yaml(&output)
        .expect("fallback parse")
        .remove(0);
    assert_eq!(fallback.config, replay.config);
    assert_eq!(fallback.tls, replay.tls);
    let mut separate = node;
    let ProtocolConfig::ShadowTls(cfg) = &mut separate.config else {
        panic!("wrapper");
    };
    let ProtocolConfig::Snell(inner) = cfg.inner_config.as_mut() else {
        panic!("Snell");
    };
    inner.obfs.as_mut().expect("alias").mode = deve_sub_domain::SnellObfsMode::Http;
    assert!(check_node(&separate, &capability_for(ProfileKind::Mihomo)).is_err());
    assert!(emit_mihomo(&[separate]).is_err());
    let mut numeric = entry;
    numeric["obfs-opts"]["password"] = json!(123456);
    assert!(matches!(
        parse(numeric).config,
        ProtocolConfig::Unsupported(_)
    ));
}
