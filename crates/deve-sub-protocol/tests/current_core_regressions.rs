//! Review regressions: format guards must prevent quiet loss of security/data.
#![allow(clippy::expect_used)]
use deve_sub_compatibility::{ProfileKind, capability_for, check_node};
use deve_sub_domain::ProtocolConfig;
use deve_sub_protocol::container::parse_singbox_json;
use serde_json::{Value, json};

fn fixtures() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/subscription-acceptance/protocols.json"
    ))
    .expect("fixtures")
}

#[test]
fn legacy_wireguard_honors_peer_endpoint_and_rejects_distinct_peers() {
    let key = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
    let mut outbound = json!({"type":"wireguard","tag":"legacy","server":"192.0.2.1","server_port":1111,"private_key":key,"local_address":["192.0.2.3/32"],"peers":[{"server":"192.0.2.2","server_port":2222,"public_key":key,"allowed_ips":["0.0.0.0/0"]}]});
    let parsed =
        parse_singbox_json(&json!({"outbounds":[outbound.clone()]}).to_string()).expect("legacy");
    assert_eq!(parsed[0].endpoint.host.uri_host(), "192.0.2.2");
    assert_eq!(parsed[0].endpoint.port, 2222);
    let mut other = outbound["peers"][0].clone();
    other["server"] = json!("192.0.2.9");
    outbound["peers"].as_array_mut().expect("peers").push(other);
    let parsed =
        parse_singbox_json(&json!({"outbounds":[outbound]}).to_string()).expect("container");
    assert!(
        matches!(&parsed[0].config, ProtocolConfig::Unsupported(c) if c.reason.contains("distinct peer endpoints"))
    );
}

#[test]
fn snell_raw_import_does_not_erase_unrepresentable_security_or_version_options() {
    let base = json!({"type":"snell","tag":"s","server":"snell.example","server_port":443,"psk":"TEST","version":4,"obfs_mode":"tls","obfs_host":"cover.example"});
    for (key, value) in [
        ("mode", json!("unshaped")),
        ("network", json!("tcp")),
        ("obfs_mode", json!(false)),
        ("tls", json!({"enabled":false})),
        ("tls", json!({"enabled":true,"insecure":false})),
        (
            "tls",
            json!({"enabled":true,"certificate_public_key_sha256":["TEST_PIN"]}),
        ),
        (
            "tls",
            json!({"enabled":true,"server_name":"different.example"}),
        ),
    ] {
        let mut entry = base.clone();
        entry[key] = value;
        let parsed = parse_singbox_json(&json!({"outbounds":[entry.clone()]}).to_string())
            .expect("container");
        assert!(
            matches!(&parsed[0].config, ProtocolConfig::Unsupported(c) if c.raw == entry),
            "lost {key}"
        );
    }
    let v6 = json!({"type":"snell","tag":"s","server":"snell.example","server_port":443,"psk":"TEST","version":6,"mode":"unshaped"});
    let nodes = parse_singbox_json(&json!({"outbounds":[v6]}).to_string()).expect("v6");
    assert!(check_node(&nodes[0], &capability_for(ProfileKind::SingBox)).is_ok());
    for target in [ProfileKind::Mihomo, ProfileKind::UriList] {
        assert!(check_node(&nodes[0], &capability_for(target)).is_err());
    }
    assert!(deve_sub_emitter::emit_uri(&nodes[0]).is_err());
    assert!(deve_sub_emitter::emit_mihomo(&nodes).is_err());
}

#[test]
fn implicit_tcp_security_survives_container_import_and_xray_emission() {
    let reality = deve_sub_protocol::parse_uri(fixtures()[0]["input"].as_str().expect("uri"))
        .expect("Reality");
    let raw = deve_sub_emitter::emit_singbox(&[reality]).expect("sing-box");
    let nodes = parse_singbox_json(&raw).expect("implicit TCP");
    assert!(nodes[0].transport.is_none());
    let emitted: Value =
        serde_json::from_str(&deve_sub_emitter::emit_xray(&nodes).expect("Xray")).expect("json");
    assert_eq!(
        emitted["outbounds"][0]["streamSettings"]["security"],
        "reality"
    );
    assert_eq!(
        emitted["outbounds"][0]["streamSettings"]["realitySettings"]["shortId"],
        "01020304"
    );
    assert!(deve_sub_emitter::emit_v2ray(&nodes).is_err());
    let nodes = parse_singbox_json(r#"{"outbounds":[{"type":"trojan","tag":"t","server":"trojan.example","server_port":443,"password":"TEST","tls":{"enabled":true,"insecure":false}}]}"#).expect("Trojan");
    let emitted: Value =
        serde_json::from_str(&deve_sub_emitter::emit_xray(&nodes).expect("Xray")).expect("json");
    assert_eq!(emitted["outbounds"][0]["streamSettings"]["security"], "tls");
    assert_eq!(
        emitted["outbounds"][0]["streamSettings"]["tlsSettings"]["allowInsecure"],
        false
    );
}

#[test]
fn shadowtls_generated_tags_do_not_collide_with_endpoints_and_inner_tls_is_preserved_raw() {
    let fixtures = fixtures();
    let mut nodes =
        parse_singbox_json(fixtures[10]["input"].as_str().expect("ShadowTLS")).expect("wrapper");
    nodes[0].display_name = "n".into();
    let mut wg =
        deve_sub_protocol::parse_uri(fixtures[7]["input"].as_str().expect("WG")).expect("wg");
    wg.display_name = "n-stls".into();
    nodes.push(wg);
    let doc: Value =
        serde_json::from_str(&deve_sub_emitter::emit_singbox(&nodes).expect("unique tags"))
            .expect("json");
    let tags = doc["outbounds"]
        .as_array()
        .expect("outbounds")
        .iter()
        .chain(doc["endpoints"].as_array().expect("endpoints"))
        .map(|v| v["tag"].as_str().expect("tag"))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(tags.len(), 3);
    assert_eq!(doc["route"]["final"], "n");
    assert_eq!(doc["outbounds"][1]["detour"], "n-stls-2");
    assert_eq!(
        parse_singbox_json(&doc.to_string())
            .expect("roundtrip")
            .len(),
        2
    );

    let mut raw: Value =
        serde_json::from_str(fixtures[10]["input"].as_str().expect("JSON")).expect("fixture");
    raw["outbounds"][1]["tls"] = json!({"enabled":true});
    let parsed = parse_singbox_json(&raw.to_string()).expect("container");
    assert!(
        matches!(&parsed[0].config, ProtocolConfig::Unsupported(c) if c.raw["tls"]["enabled"] == true)
    );
    raw["outbounds"][1] = json!({"type":"vmess","tag":"inner-ws","server":"stls.example","server_port":443,"uuid":"00000000-0000-4000-8000-000000000001","security":"auto","detour":"wrapper","transport":{"type":"ws","path":"/inner","headers":{"Host":"front.example"}}});
    let ws = parse_singbox_json(&raw.to_string()).expect("inner WS");
    assert_eq!(
        ws[0].transport.as_ref().expect("preserved transport").kind,
        deve_sub_domain::TransportKind::Ws
    );
    let emitted: Value =
        serde_json::from_str(&deve_sub_emitter::emit_singbox(&ws).expect("emit WS")).expect("json");
    assert_eq!(
        emitted["outbounds"][1]["transport"],
        raw["outbounds"][1]["transport"]
    );
    assert_eq!(
        parse_singbox_json(&emitted.to_string()).expect("roundtrip")[0].transport,
        ws[0].transport
    );
    let mut wrapper =
        deve_sub_protocol::parse_uri("shadow-tls://TEST@wrapper.example:443?version=3")
            .expect("wrapper");
    wrapper.tls.as_mut().expect("TLS").alpn = vec!["h2".into()];
    assert!(deve_sub_emitter::emit_uri(&wrapper).is_err());
}
