//! Regression oracles for the pinned 2026 client schemas, independent of emitters.
#![allow(clippy::expect_used)]
use deve_sub_compatibility::{ProfileKind, capability_for, check_node};
use deve_sub_domain::{Node, ProtocolConfig};
use deve_sub_protocol::container::{parse_auto_json, parse_singbox_json};
use serde_json::{Value, json};

fn endpoint() -> Value {
    json!({"type":"wireguard","tag":"wg-first","private_key":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
        "address":["192.0.2.2/32","2001:db8::2/128"], "mtu":1408,"workers":2,
        "peers":[{"address":"2001:db8::1","port":51820,"public_key":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
            "pre_shared_key":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=","allowed_ips":["0.0.0.0/0","::/0"],
            "reserved":[0,127,255],"persistent_keepalive_interval":25}]})
}

fn typed(doc: &Value) -> Vec<Node> {
    let nodes = parse_auto_json(&doc.to_string()).expect("parse auto");
    assert!(
        nodes
            .iter()
            .all(|n| !matches!(n.config, ProtocolConfig::Unsupported(_))),
        "{nodes:?}"
    );
    nodes
}

#[test]
fn endpoint_only_ipv6_preserves_peer_fields_and_routes_through_endpoint() {
    let before = endpoint();
    let nodes = typed(&json!({"endpoints":[before]}));
    let emitted = deve_sub_emitter::emit_singbox(&nodes).expect("emit");
    let raw: Value = serde_json::from_str(&emitted).expect("json");
    assert_eq!(raw["endpoints"][0], before);
    assert_eq!(raw["outbounds"], json!([]));
    assert_eq!(raw["route"]["final"], "wg-first");
    let reparsed = parse_singbox_json(&emitted).expect("parse explicit");
    assert_eq!(reparsed[0].config, nodes[0].config);
    assert_eq!(reparsed[0].endpoint, nodes[0].endpoint);

    let trojan =
        deve_sub_protocol::parse_uri("trojan://TEST@trojan.example:443#second").expect("trojan");
    for (batch, first) in [
        (vec![nodes[0].clone(), trojan.clone()], "wg-first"),
        (vec![trojan, nodes[0].clone()], "second"),
    ] {
        let raw: Value =
            serde_json::from_str(&deve_sub_emitter::emit_singbox(&batch).expect("mixed"))
                .expect("json");
        assert_eq!(raw["route"]["final"], first);
        assert_eq!(raw["outbounds"].as_array().expect("outbounds").len(), 1);
        assert_eq!(typed(&raw).len(), 2);
    }
}

#[test]
fn endpoint_import_rejects_lossy_options_and_invalid_numeric_shapes() {
    for (pointer, value) in [
        ("/system", json!(true)),
        ("/listen_port", json!(51820)),
        ("/bind_interface", json!("eth-test")),
        ("/address", json!([1])),
        ("/address", json!([])),
        ("/address", json!(["not-a-prefix"])),
        ("/address", json!(["2001:db8::1/129"])),
        ("/private_key", json!("")),
        ("/private_key", json!("not-base64")),
        ("/peers/0/public_key", json!("YQ==")),
        ("/peers/0/pre_shared_key", json!("")),
        ("/peers/0/allowed_ips", json!(["192.0.2.0/999"])),
        ("/mtu", json!(4294967296_u64)),
        ("/workers", json!(-1)),
        ("/peers/0/port", json!(65536)),
        ("/peers/0/port", json!(0)),
        ("/peers/0/persistent_keepalive_interval", json!(65536)),
        ("/peers/0/persistent_keepalive_interval", json!(1.5)),
        ("/peers/0/reserved", json!([1, 2, 256])),
        ("/peers/0/reserved", json!([1, 2])),
        ("/peers/0/allowed_ips", json!([true])),
        ("/peers", json!([])),
    ] {
        let mut value_endpoint = endpoint();
        if let Some(key) = pointer.strip_prefix('/').filter(|k| !k.contains('/')) {
            value_endpoint[key] = value;
        } else {
            *value_endpoint.pointer_mut(pointer).expect("existing field") = value;
        }
        let nodes = parse_auto_json(&json!({"endpoints":[value_endpoint.clone()]}).to_string())
            .expect("container");
        let ProtocolConfig::Unsupported(reason) = &nodes[0].config else {
            panic!("accepted {pointer}")
        };
        assert_eq!(reason.raw, value_endpoint);
        assert!(!reason.reason.is_empty());
    }
    let mut peers = endpoint();
    let mut other = peers["peers"][0].clone();
    other["address"] = json!("192.0.2.9");
    peers["peers"].as_array_mut().expect("peers").push(other);
    let nodes = parse_auto_json(&json!({"endpoints":[peers]}).to_string()).expect("container");
    assert!(
        matches!(&nodes[0].config, ProtocolConfig::Unsupported(reason) if reason.reason.contains("distinct peer endpoints"))
    );
}

#[test]
fn combined_raw_entry_limit_precedes_parsing_or_wrapper_merging() {
    let doc =
        json!({"outbounds":vec![json!({"type":"future"});5000], "endpoints":vec![endpoint();5001]});
    for parse in [parse_auto_json, parse_singbox_json] {
        assert!(matches!(
            parse(&doc.to_string()),
            Err(deve_sub_protocol::ParseError::TooManyEntries(10001))
        ));
        for invalid in [
            json!({"endpoints":null}),
            json!({"outbounds":{},"endpoints":[]}),
            json!({"endpoints":{},"outbounds":[]}),
        ] {
            assert!(parse(&invalid.to_string()).is_err());
        }
    }
    assert_eq!(parse_auto_json(&json!({"outbounds":vec![json!({"type":"future"});5000],"endpoints":vec![json!({"type":"future"});5000]}).to_string()).expect("at limit").len(), 10000);
}

#[test]
fn snell_camouflage_uses_obfs_fields_and_extra_tls_is_rejected() {
    let node = deve_sub_protocol::parse_uri(
        "snell://TEST@snell.example:443?version=4&obfs=tls&obfs-host=cover.example#snell",
    )
    .expect("snell");
    check_node(&node, &capability_for(ProfileKind::SingBox)).expect("camouflage supported");
    let raw: Value = serde_json::from_str(
        &deve_sub_emitter::emit_singbox(std::slice::from_ref(&node)).expect("emit"),
    )
    .expect("json");
    assert_eq!(raw["outbounds"][0]["obfs_mode"], "tls");
    assert_eq!(raw["outbounds"][0]["obfs_host"], "cover.example");
    assert!(raw["outbounds"][0].get("tls").is_none());
    assert_eq!(typed(&raw)[0].tls, node.tls);
    for verification in [Some(false), Some(true)] {
        let mut extra = node.clone();
        extra.tls.as_mut().expect("tls alias").skip_cert_verify = verification;
        assert!(check_node(&extra, &capability_for(ProfileKind::SingBox)).is_err());
        assert!(deve_sub_emitter::emit_singbox(&[extra]).is_err());
    }
    let mut userkey = node.clone();
    userkey
        .extras
        .insert("snell_userkey".into(), json!("TEST_USERKEY"));
    for profile in [ProfileKind::Mihomo, ProfileKind::UriList] {
        assert!(check_node(&userkey, &capability_for(profile)).is_err());
    }
    let mut raw6 = raw;
    raw6["outbounds"][0]["version"] = json!(6);
    raw6["outbounds"][0]["mode"] = json!("unknown");
    assert!(matches!(
        parse_singbox_json(&raw6.to_string()).expect("container")[0].config,
        ProtocolConfig::Unsupported(_)
    ));
}

#[test]
fn reality_and_shadowtls_security_do_not_silently_downgrade() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/subscription-acceptance/protocols.json"
    ))
    .expect("fixtures");
    let reality =
        deve_sub_protocol::parse_uri(fixtures[0]["input"].as_str().expect("URI")).expect("Reality");
    assert!(check_node(&reality, &capability_for(ProfileKind::V2Ray)).is_err());
    assert!(deve_sub_emitter::emit_v2ray(std::slice::from_ref(&reality)).is_err());
    assert!(
        deve_sub_emitter::emit_xray(&[reality])
            .expect("Xray")
            .contains("realitySettings")
    );
    let complete =
        parse_singbox_json(fixtures[10]["input"].as_str().expect("container")).expect("ShadowTLS");
    assert!(deve_sub_emitter::emit_uri(&complete[0]).is_err());
    assert!(check_node(&complete[0], &capability_for(ProfileKind::UriList)).is_err());
    for (query, verify) in [
        ("", None),
        ("&insecure=0", Some(false)),
        ("&insecure=1", Some(true)),
    ] {
        let node = deve_sub_protocol::parse_uri(&format!(
            "shadow-tls://TEST@wrapper.example:443?version=3{query}"
        ))
        .expect("wrapper");
        assert_eq!(node.tls.as_ref().expect("tls").skip_cert_verify, verify);
        let roundtrip = deve_sub_protocol::parse_uri(
            &deve_sub_emitter::emit_uri(&node).expect("wrapper utility"),
        )
        .expect("parse");
        assert_eq!(roundtrip.tls, node.tls);
        for profile in [
            ProfileKind::Mihomo,
            ProfileKind::SingBox,
            ProfileKind::UriList,
        ] {
            assert!(check_node(&node, &capability_for(profile)).is_err());
        }
    }
}
