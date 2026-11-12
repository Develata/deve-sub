//! Independent native-schema assertions for CA01–CA06 and TUIC time units.
#![allow(clippy::expect_used)]

use deve_sub_compatibility::{ProfileKind, capability_for, check_node};
use deve_sub_domain::{Authentication, CertificatePin, Node, ProtocolConfig};
use deve_sub_emitter::{emit_mihomo, emit_singbox, emit_uri, emit_v2ray, emit_xray};
use deve_sub_protocol::{
    container::{parse_mihomo_yaml, parse_singbox_json},
    parse_uri,
};
use serde_json::Value;

const PIN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const UUID: &str = "00000000-0000-4000-8000-000000000001";

fn uri(value: &str) -> Node {
    parse_uri(value).expect("reserved URI fixture")
}

fn mihomo_proxy(node: &Node) -> Value {
    let output = emit_mihomo(std::slice::from_ref(node)).expect("Mihomo emission");
    deve_sub_yaml::parse_source_json(&output).expect("emitted YAML")["proxies"][0].clone()
}

fn singbox_outbound(node: &Node) -> Value {
    let output = emit_singbox(std::slice::from_ref(node)).expect("sing-box emission");
    serde_json::from_str::<Value>(&output).expect("emitted JSON")["outbounds"][0].clone()
}

#[test]
fn ca01_native_server_fields_use_bare_ipv6() {
    let node = uri("trojan://TEST_PASSWORD@[2001:db8::1]:443#ipv6");
    assert_eq!(mihomo_proxy(&node)["server"], "2001:db8::1");
    assert_eq!(singbox_outbound(&node)["server"], "2001:db8::1");
    for output in [
        emit_xray(std::slice::from_ref(&node)),
        emit_v2ray(std::slice::from_ref(&node)),
    ] {
        let value: Value = serde_json::from_str(&output.expect("native JSON")).expect("JSON");
        assert_eq!(
            value["outbounds"][0]["settings"]["servers"][0]["address"],
            "2001:db8::1"
        );
    }
    assert!(emit_uri(&node).expect("URI").contains("@[2001:db8::1]:443"));
}

#[test]
fn ca02_singbox_grpc_uses_service_name() {
    let node = parse_singbox_json(r#"{"outbounds":[{"type":"trojan","tag":"grpc","server":"example.com","server_port":443,"password":"TEST_PASSWORD","tls":{"enabled":true},"transport":{"type":"grpc","service_name":"fixture-service"}}]}"#).expect("native gRPC fixture").remove(0);
    let outbound = singbox_outbound(&node);
    assert_eq!(outbound["transport"]["service_name"], "fixture-service");
    assert!(outbound["transport"].get("path").is_none());
}

#[test]
fn ca03_mihomo_numeric_name_and_password_are_text() {
    let nodes = parse_mihomo_yaml("proxies:\n  - {name: 1234, type: trojan, server: example.com, port: 443, password: 123456}\n").expect("YAML");
    assert_eq!(nodes[0].display_name, "1234");
    assert_eq!(
        nodes[0].authentication,
        Authentication::Password {
            password: "123456".into()
        }
    );
}

#[test]
fn ca03_mihomo_numeric_short_id_and_existing_leading_zero_are_text() {
    for short_id in ["12345678", "01020304"] {
        let yaml = format!(
            "proxies:\n  - name: reality\n    type: vless\n    server: example.com\n    port: 443\n    uuid: {UUID}\n    reality-opts:\n      public-key: AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8\n      short-id: {short_id}\n"
        );
        let nodes = parse_mihomo_yaml(&yaml).expect("YAML");
        assert_eq!(
            nodes[0]
                .tls
                .as_ref()
                .expect("TLS")
                .reality
                .as_ref()
                .expect("Reality")
                .short_id,
            short_id
        );
    }
}

#[test]
fn ca04_mihomo_retains_its_native_fingerprint_policy() {
    let node = parse_mihomo_yaml(&format!("proxies:\n  - {{name: pin, type: hysteria2, server: example.com, port: 443, password: TEST_PASSWORD, skip-cert-verify: true, fingerprint: {PIN}}}\n")).expect("native pin").remove(0);
    let proxy = mihomo_proxy(&node);
    assert_eq!(proxy["fingerprint"], PIN);
    assert_eq!(proxy["skip-cert-verify"], true);
    let reparsed = parse_mihomo_yaml(&emit_mihomo(&[node]).expect("emit")).expect("parse");
    assert_eq!(
        reparsed[0].tls.as_ref().expect("TLS").certificate_pins[0].as_str(),
        format!("mihomoFingerprintSHA256:{PIN}")
    );
    assert!(emit_singbox(&reparsed).is_err());
    assert!(emit_uri(&reparsed[0]).is_err());
}

#[test]
fn ca04_native_targets_reject_hysteria_leaf_pin_trust_policy() {
    for insecure in ["", "&insecure=0", "&insecure=1"] {
        let node = uri(&format!(
            "hysteria2://TEST_PASSWORD@example.com:443?pinSHA256={PIN}{insecure}#pin"
        ));
        assert!(emit_singbox(std::slice::from_ref(&node)).is_err());
        assert!(emit_mihomo(std::slice::from_ref(&node)).is_err());
        for profile in [ProfileKind::SingBox, ProfileKind::Mihomo] {
            assert!(check_node(&node, &capability_for(profile)).is_err());
        }
        let reparsed = parse_uri(&emit_uri(&node).expect("leaf URI preserved")).expect("URI");
        assert_eq!(node.tls, reparsed.tls);
    }
}

#[test]
fn ca04_native_and_uri_emitters_reject_unrepresentable_pin_types() {
    let mut node = uri("trojan://TEST_PASSWORD@example.com:443#pin");
    node.tls.as_mut().expect("TLS").certificate_pins =
        vec![CertificatePin::new("unknown:fixture".into())];
    for profile in [
        ProfileKind::Mihomo,
        ProfileKind::SingBox,
        ProfileKind::Xray,
        ProfileKind::V2Ray,
        ProfileKind::UriList,
        ProfileKind::Shadowrocket,
    ] {
        assert!(
            check_node(&node, &capability_for(profile)).is_err(),
            "{profile:?}"
        );
    }
    assert!(emit_mihomo(std::slice::from_ref(&node)).is_err());
    assert!(emit_singbox(std::slice::from_ref(&node)).is_err());
    assert!(emit_xray(std::slice::from_ref(&node)).is_err());
    assert!(emit_v2ray(std::slice::from_ref(&node)).is_err());
    assert!(emit_uri(&node).is_err());
}

#[test]
fn ca04_singbox_retains_spki_pin() {
    let node = parse_singbox_json(r#"{"outbounds":[{"type":"trojan","tag":"pin","server":"example.com","server_port":443,"password":"TEST_PASSWORD","tls":{"enabled":true,"insecure":true,"certificate_public_key_sha256":["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="]}}]}"#).expect("JSON").remove(0);
    assert_eq!(node.tls.as_ref().expect("TLS").certificate_pins.len(), 1);
    assert_eq!(
        singbox_outbound(&node)["tls"]["certificate_public_key_sha256"][0],
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
    );
}

#[test]
fn ca05_mihomo_obfs_options_map_to_sip003_and_back() {
    let node = parse_mihomo_yaml("proxies:\n  - name: obfs\n    type: ss\n    server: example.com\n    port: 443\n    cipher: aes-128-gcm\n    password: TEST_PASSWORD\n    plugin: obfs\n    plugin-opts:\n      mode: http\n      host: camouflage.example.com\n").expect("YAML").remove(0);
    let ProtocolConfig::Shadowsocks(config) = &node.config else {
        panic!("Shadowsocks")
    };
    assert_eq!(config.plugin.as_deref(), Some("obfs-local"));
    let opts = config.plugin_opts.as_deref().expect("plugin opts");
    assert!(opts.contains("obfs=http"));
    assert!(opts.contains("obfs-host=camouflage.example.com"));
    let proxy = mihomo_proxy(&node);
    assert_eq!(proxy["plugin"], "obfs");
    assert_eq!(proxy["plugin-opts"]["mode"], "http");
    assert_eq!(proxy["plugin-opts"]["host"], "camouflage.example.com");
    assert_eq!(singbox_outbound(&node)["plugin"], "obfs-local");
}

#[test]
fn ca05_uri_obfs_local_maps_native_mihomo_fields() {
    let node = uri(
        "ss://YWVzLTEyOC1nY206VEVTVF9QQVNTV09SRA@example.com:443/?plugin=obfs-local%3Bobfs%3Dtls%3Bobfs-host%3Dcamouflage.example.com#obfs",
    );
    let proxy = mihomo_proxy(&node);
    assert_eq!(proxy["plugin"], "obfs");
    assert_eq!(proxy["plugin-opts"]["mode"], "tls");
    assert_eq!(proxy["plugin-opts"]["host"], "camouflage.example.com");
}

#[test]
fn ca05_unsupported_plugin_options_are_explicit() {
    let yaml = "proxies:\n  - {name: unsupported, type: ss, server: example.com, port: 443, cipher: aes-128-gcm, password: TEST_PASSWORD, plugin: obfs, plugin-opts: {mode: http, unsupported: value}}\n";
    let node = parse_mihomo_yaml(yaml).expect("YAML").remove(0);
    assert!(matches!(node.config, ProtocolConfig::Unsupported(_)));
    let node = uri(
        "ss://YWVzLTEyOC1nY206VEVTVF9QQVNTV09SRA@example.com:443/?plugin=unknown-plugin%3Bsetting%3Dfixture#unsupported",
    );
    for profile in [
        ProfileKind::Mihomo,
        ProfileKind::SingBox,
        ProfileKind::Xray,
        ProfileKind::V2Ray,
    ] {
        assert!(
            check_node(&node, &capability_for(profile)).is_err(),
            "{profile:?}"
        );
    }
    assert!(emit_mihomo(std::slice::from_ref(&node)).is_err());
    assert!(emit_singbox(std::slice::from_ref(&node)).is_err());
    assert!(emit_xray(std::slice::from_ref(&node)).is_err());
    assert!(emit_v2ray(&[node]).is_err());
}

#[test]
fn ca05_native_empty_plugin_host_is_not_replaced_by_default() {
    for (plugin, mode) in [("obfs", "tls"), ("v2ray-plugin", "websocket")] {
        let yaml = format!(
            "proxies:\n  - {{name: empty-host, type: ss, server: example.com, port: 443, cipher: aes-128-gcm, password: TEST_PASSWORD, plugin: {plugin}, plugin-opts: {{mode: {mode}, host: ''}}}}\n"
        );
        let node = parse_mihomo_yaml(&yaml).expect("native fixture").remove(0);
        assert!(
            matches!(node.config, ProtocolConfig::Unsupported(_)),
            "explicit empty {plugin} host cannot become bing.com"
        );
    }
}

#[test]
fn ca06_singbox_rejects_lossy_bandwidth_and_retains_integer_mbps() {
    for bandwidth in ["100bps", "500Kbps", "1.5Mbps"] {
        let node = uri(&format!(
            "hysteria2://TEST_PASSWORD@example.com:443?up={bandwidth}#rate"
        ));
        assert!(
            emit_singbox(std::slice::from_ref(&node)).is_err(),
            "{bandwidth}"
        );
        assert!(
            check_node(&node, &capability_for(ProfileKind::SingBox)).is_err(),
            "{bandwidth}"
        );
    }
    let node = uri("hysteria2://TEST_PASSWORD@example.com:443?up=100Mbps&down=200Mbps#rate");
    assert_eq!(singbox_outbound(&node)["up_mbps"], 100);
    assert_eq!(singbox_outbound(&node)["down_mbps"], 200);
}

#[test]
fn ca06_explicit_bps_and_absent_bandwidth_are_preserved() {
    let node = uri("hysteria2://TEST_PASSWORD@example.com:443?up=100bps#rate");
    assert_eq!(
        node.congestion.as_ref().expect("congestion").up_bps,
        Some(100)
    );
    assert_eq!(mihomo_proxy(&node)["up"], "100 bps");
    let node = uri("hysteria2://TEST_PASSWORD@example.com:443#default-rate");
    let outbound = singbox_outbound(&node);
    assert!(outbound.get("up_mbps").is_none());
    assert!(outbound.get("down_mbps").is_none());
}

#[test]
fn ca06_mihomo_unitless_bandwidth_defaults_to_mbps() {
    for value in ["100", "\"100\"", "\"100bps\"", "\"500Kbps\""] {
        let nodes = parse_mihomo_yaml(&format!("proxies:\n  - {{name: rate, type: hysteria2, server: example.com, port: 443, password: TEST_PASSWORD, up: {value}}}\n")).expect("YAML");
        let expected = match value {
            "\"100bps\"" => 100,
            "\"500Kbps\"" => 500_000,
            _ => 100_000_000,
        };
        assert_eq!(
            nodes[0].congestion.as_ref().expect("bandwidth").up_bps,
            Some(expected),
            "{value}"
        );
    }
}

#[test]
fn tuic_mihomo_heartbeat_interval_is_milliseconds() {
    let node = uri(&format!(
        "tuic://{UUID}:TEST_PASSWORD@example.com:443?heartbeat=10500#heartbeat"
    ));
    assert_eq!(mihomo_proxy(&node)["heartbeat-interval"], 10500);
    let yaml = format!(
        "proxies:\n  - {{name: heartbeat, type: tuic, server: example.com, port: 443, uuid: {UUID}, password: TEST_PASSWORD, heartbeat-interval: 10500}}\n"
    );
    let nodes = parse_mihomo_yaml(&yaml).expect("YAML");
    let ProtocolConfig::TuicV5(config) = &nodes[0].config else {
        panic!("TUIC")
    };
    assert_eq!(config.heartbeat, Some(time::Duration::milliseconds(10500)));
}
