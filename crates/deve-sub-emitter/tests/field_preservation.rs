//! W-Q/W-R/W-S regression tests: verify mihomo and sing-box emitters
//! preserve Reality fields, client-fingerprint, and transport Host header
//! that were previously silently dropped.

#![allow(clippy::expect_used)]

mod common;

use deve_sub_domain::{
    CongestionConfig, CongestionController, Node, Obfuscation, ProtocolConfig, ShadowsocksConfig,
    Transport, TransportKind,
};
use deve_sub_emitter::{emit_mihomo, emit_singbox};

use common::sample_nodes;

fn find_node(name: &str) -> Node {
    sample_nodes()
        .into_iter()
        .find(|n| n.display_name == name)
        .expect("fixture node exists")
}

/// W-Q: mihomo emitter must emit reality-opts with public-key and short-id.
#[test]
fn mihomo_emits_reality_opts() {
    let node = find_node("vless-reality-test");
    let output = emit_mihomo(&[node]).expect("emit");
    assert!(output.contains("reality-opts:"), "must emit reality-opts");
    assert!(
        output.contains("public-key:"),
        "must emit public-key under reality-opts"
    );
    assert!(
        output.contains("short-id:"),
        "must emit short-id under reality-opts"
    );
}

/// W-Q: sing-box emitter must emit reality block with public_key and short_id.
#[test]
fn singbox_emits_reality_block() {
    let node = find_node("vless-reality-test");
    let output = emit_singbox(&[node]).expect("emit");
    assert!(output.contains("\"reality\""), "must emit reality block");
    assert!(
        output.contains("\"public_key\""),
        "must emit public_key in reality block"
    );
    assert!(
        output.contains("\"short_id\""),
        "must emit short_id in reality block"
    );
}

/// W-S: mihomo emitter must emit client-fingerprint when set.
#[test]
fn mihomo_emits_client_fingerprint() {
    let node = find_node("vless-reality-test");
    let output = emit_mihomo(&[node]).expect("emit");
    assert!(
        output.contains("client-fingerprint:"),
        "must emit client-fingerprint"
    );
    assert!(output.contains("chrome"), "must emit the fingerprint value");
}

/// W-R: mihomo emitter must emit Ws headers.Host when transport.host is set.
#[test]
fn mihomo_emits_ws_host_header() {
    let node = find_node("trojan-test");
    let output = emit_mihomo(&[node]).expect("emit");
    assert!(
        output.contains("headers:"),
        "must emit headers block for Ws transport with host"
    );
    assert!(
        output.contains("Host:"),
        "must emit Host header under headers"
    );
}

/// W-R: mihomo emitter must emit H2 host as a list.
#[test]
fn mihomo_emits_h2_host_list() {
    let mut node = find_node("trojan-test");
    node.transport = Some(Transport {
        kind: TransportKind::H2,
        path: Some("/h2".to_owned()),
        host: Some("h2.example.com".to_owned()),
        xhttp_mode: None,
    });
    let output = emit_mihomo(&[node]).expect("emit");
    assert!(output.contains("h2-opts:"), "must emit h2-opts");
    assert!(
        output.contains("host: ["),
        "must emit host as YAML list for H2 transport"
    );
}

/// P3-1: mihomo emitter must emit packet-encoding when set.
#[test]
fn mihomo_emits_packet_encoding() {
    let node = find_node("vmess-test");
    let output = emit_mihomo(&[node]).expect("emit");
    assert!(
        output.contains("packet-encoding:"),
        "must emit packet-encoding for vmess with packet_encoding set"
    );
}

/// P3-1: sing-box emitter must emit packet_encoding when set.
#[test]
fn singbox_emits_packet_encoding() {
    let node = find_node("vmess-test");
    let output = emit_singbox(&[node]).expect("emit");
    assert!(
        output.contains("\"packet_encoding\""),
        "must emit packet_encoding for vmess with packet_encoding set"
    );
}

/// P3-4: mihomo emitter must emit v2ray-http-upgrade: true for HttpUpgrade.
#[test]
fn mihomo_emits_httpupgrade_flag() {
    let mut node = find_node("trojan-test");
    node.transport = Some(Transport {
        kind: TransportKind::HttpUpgrade,
        path: Some("/hu".to_owned()),
        host: None,
        xhttp_mode: None,
    });
    let output = emit_mihomo(&[node]).expect("emit");
    assert!(
        output.contains("v2ray-http-upgrade: true"),
        "must emit v2ray-http-upgrade: true for HttpUpgrade transport"
    );
}

// =========================================================================
// P1-1 field-loss regression tests (E1, E2, E3a, E3b)
// =========================================================================

/// E1: mihomo emit_ss must emit `plugin` and `plugin-opts` when set.
#[test]
fn mihomo_emits_ss_plugin() {
    let mut node = find_node("ss-test");
    node.config = ProtocolConfig::Shadowsocks(ShadowsocksConfig {
        method: "aes-256-gcm".to_owned(),
        plugin: Some("v2ray-plugin".to_owned()),
        plugin_opts: Some("mode=websocket;host=example.com;path=/ws;tls=true".to_owned()),
    });
    let output = emit_mihomo(&[node]).expect("emit");
    assert!(
        output.contains("plugin: v2ray-plugin"),
        "must emit plugin name"
    );
    assert!(
        output.contains("plugin-opts:"),
        "must emit plugin-opts block"
    );
    assert!(
        output.contains("mode: \"websocket\""),
        "must emit mode from opts"
    );
    assert!(
        output.contains("host: \"example.com\""),
        "must emit host from opts"
    );
    assert!(output.contains("path: \"/ws\""), "must emit path from opts");
    assert!(output.contains("tls: true"), "must emit tls as bare bool");
}

/// E2: mihomo emit_hysteria2 must emit ports, hop-interval, up, down, obfs.
#[test]
fn mihomo_emits_hysteria2_full_fields() {
    let mut node = find_node("hy2-test");
    node.config = ProtocolConfig::Hysteria2(deve_sub_domain::Hysteria2Config {
        ports: Some("20000-40000".to_owned()),
        hop_interval: Some(time::Duration::seconds(30)),
        fast_open: None,
        lazy: None,
    });
    node.congestion = Some(CongestionConfig {
        controller: CongestionController::Bbr,
        up_bps: Some(100_000_000),
        down_bps: Some(200_000_000),
    });
    node.obfuscation = Some(Obfuscation {
        kind: "salamander".to_owned(),
        password: Some("obfs-pwd".to_owned()),
    });
    let output = emit_mihomo(&[node]).expect("emit");
    assert!(output.contains("ports: \"20000-40000\""), "must emit ports");
    assert!(
        output.contains("hop-interval: 30"),
        "must emit hop-interval"
    );
    assert!(
        output.contains("up: \"100 Mbps\""),
        "must emit up bandwidth"
    );
    assert!(
        output.contains("down: \"200 Mbps\""),
        "must emit down bandwidth"
    );
    assert!(output.contains("obfs: salamander"), "must emit obfs kind");
    assert!(
        output.contains("obfs-password: \"obfs-pwd\""),
        "must emit obfs password"
    );
}

/// E3b-mihomo: mihomo emit_tuic_v5 must emit congestion, udp-relay-mode,
/// reduce-rtt, heartbeat-interval, disable-sni.
#[test]
fn mihomo_emits_tuic_v5_full_fields() {
    let mut node = find_node("tuic-test");
    node.config = ProtocolConfig::TuicV5(deve_sub_domain::TuicV5Config {
        udp_relay_mode: Some(deve_sub_domain::UdpRelayMode::Native),
        zero_rtt_handshake: Some(true),
        heartbeat: Some(time::Duration::seconds(10)),
        disable_sni: Some(true),
    });
    node.congestion = Some(CongestionConfig {
        controller: CongestionController::Bbr,
        up_bps: None,
        down_bps: None,
    });
    let output = emit_mihomo(&[node]).expect("emit");
    assert!(
        output.contains("congestion-controller: bbr"),
        "must emit congestion controller"
    );
    assert!(
        output.contains("udp-relay-mode: native"),
        "must emit udp-relay-mode"
    );
    assert!(
        output.contains("reduce-rtt: true"),
        "must emit reduce-rtt (not zero-rtt-handshake)"
    );
    assert!(
        output.contains("heartbeat-interval: 10"),
        "must emit heartbeat-interval in seconds"
    );
    assert!(
        output.contains("disable-sni: true"),
        "must emit disable-sni"
    );
}

/// E3a: sing-box emitter must emit up_mbps, down_mbps, server_ports,
/// hop_interval, and obfs as a nested object.
#[test]
fn singbox_emits_hysteria2_full_fields() {
    let mut node = find_node("hy2-test");
    node.config = ProtocolConfig::Hysteria2(deve_sub_domain::Hysteria2Config {
        ports: Some("20000-40000".to_owned()),
        hop_interval: Some(time::Duration::seconds(30)),
        fast_open: None,
        lazy: None,
    });
    node.congestion = Some(CongestionConfig {
        controller: CongestionController::Bbr,
        up_bps: Some(100_000_000),
        down_bps: Some(200_000_000),
    });
    node.obfuscation = Some(Obfuscation {
        kind: "salamander".to_owned(),
        password: Some("obfs-pwd".to_owned()),
    });
    let output = emit_singbox(&[node]).expect("emit");
    assert!(
        output.contains("\"up_mbps\": 100"),
        "must emit up_mbps as int Mbps"
    );
    assert!(
        output.contains("\"down_mbps\": 200"),
        "must emit down_mbps as int Mbps"
    );
    assert!(
        output.contains("\"server_ports\": \"20000-40000\""),
        "must emit server_ports"
    );
    assert!(
        output.contains("\"hop_interval\": \"30s\""),
        "must emit hop_interval as Go duration"
    );
    assert!(
        output.contains("\"obfs\": {"),
        "must emit obfs as nested object"
    );
    assert!(
        output.contains("\"type\": \"salamander\""),
        "must emit obfs.type"
    );
    assert!(
        output.contains("\"password\": \"obfs-pwd\""),
        "must emit obfs.password"
    );
}

/// E3b: sing-box emitter must emit congestion_control, udp_relay_mode,
/// zero_rtt_handshake, heartbeat. Must NOT emit disable_sni (sing-box has
/// no such field).
#[test]
fn singbox_emits_tuic_v5_full_fields() {
    let mut node = find_node("tuic-test");
    node.config = ProtocolConfig::TuicV5(deve_sub_domain::TuicV5Config {
        udp_relay_mode: Some(deve_sub_domain::UdpRelayMode::Native),
        zero_rtt_handshake: Some(true),
        heartbeat: Some(time::Duration::seconds(10)),
        disable_sni: Some(true),
    });
    node.congestion = Some(CongestionConfig {
        controller: CongestionController::Bbr,
        up_bps: None,
        down_bps: None,
    });
    let output = emit_singbox(&[node]).expect("emit");
    assert!(
        output.contains("\"congestion_control\": \"bbr\""),
        "must emit congestion_control"
    );
    assert!(
        output.contains("\"udp_relay_mode\": \"native\""),
        "must emit udp_relay_mode"
    );
    assert!(
        output.contains("\"zero_rtt_handshake\": true"),
        "must emit zero_rtt_handshake"
    );
    assert!(
        output.contains("\"heartbeat\": \"10s\""),
        "must emit heartbeat as Go duration"
    );
    assert!(
        !output.contains("disable_sni"),
        "must NOT emit disable_sni (sing-box has no such field)"
    );
}

/// Round-trip: sing-box → canonical → sing-box preserves hysteria2 fields.
#[test]
fn singbox_hysteria2_round_trip() {
    let json = format!(
        r#"{{"outbounds": [{{"type":"hysteria2","tag":"hy2-rt","server":"hy2.example.com","server_port":443,"password":"{PWD}","up_mbps":100,"down_mbps":200,"server_ports":"20000-40000","hop_interval":"30s","obfs":{{"type":"salamander","password":"obfs-pwd"}},"tls":{{"enabled":true,"server_name":"hy2.example.com"}}}}]}}"#,
        PWD = common::RESERVED_PASSWORD
    );
    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);
    let node = &nodes[0];

    let ProtocolConfig::Hysteria2(cfg) = &node.config else {
        panic!("expected Hysteria2 config");
    };
    assert_eq!(cfg.ports.as_deref(), Some("20000-40000"));
    assert_eq!(cfg.hop_interval, Some(time::Duration::seconds(30)));
    let cong = node.congestion.as_ref().expect("congestion");
    assert_eq!(cong.up_bps, Some(100_000_000));
    assert_eq!(cong.down_bps, Some(200_000_000));
    let obfs = node.obfuscation.as_ref().expect("obfs");
    assert_eq!(obfs.kind, "salamander");
    assert_eq!(obfs.password.as_deref(), Some("obfs-pwd"));

    let re_emitted = emit_singbox(std::slice::from_ref(node)).expect("emit");
    assert!(re_emitted.contains("\"up_mbps\": 100"));
    assert!(re_emitted.contains("\"down_mbps\": 200"));
    assert!(re_emitted.contains("\"server_ports\": \"20000-40000\""));
    assert!(re_emitted.contains("\"hop_interval\": \"30s\""));
    assert!(re_emitted.contains("\"type\": \"salamander\""));
}

/// Round-trip: sing-box → canonical → sing-box preserves tuic v5 fields.
#[test]
fn singbox_tuic_v5_round_trip() {
    let json = format!(
        r#"{{"outbounds": [{{"type":"tuic","tag":"tuic-rt","server":"tuic.example.com","server_port":443,"uuid":"{UUID}","password":"{PWD}","congestion_control":"bbr","udp_relay_mode":"native","zero_rtt_handshake":true,"heartbeat":"10s","tls":{{"enabled":true,"server_name":"tuic.example.com"}}}}]}}"#,
        UUID = common::RESERVED_UUID,
        PWD = common::RESERVED_PASSWORD
    );
    let nodes = deve_sub_protocol::container::parse_singbox_json(&json).expect("parse");
    assert_eq!(nodes.len(), 1);
    let node = &nodes[0];

    let ProtocolConfig::TuicV5(cfg) = &node.config else {
        panic!("expected TuicV5 config");
    };
    assert_eq!(
        cfg.udp_relay_mode,
        Some(deve_sub_domain::UdpRelayMode::Native)
    );
    assert_eq!(cfg.zero_rtt_handshake, Some(true));
    assert_eq!(cfg.heartbeat, Some(time::Duration::seconds(10)));
    assert_eq!(
        cfg.disable_sni, None,
        "sing-box TUIC has no disable_sni field"
    );
    let cong = node.congestion.as_ref().expect("congestion");
    assert!(matches!(cong.controller, CongestionController::Bbr));

    let re_emitted = emit_singbox(std::slice::from_ref(node)).expect("emit");
    assert!(re_emitted.contains("\"congestion_control\": \"bbr\""));
    assert!(re_emitted.contains("\"udp_relay_mode\": \"native\""));
    assert!(re_emitted.contains("\"zero_rtt_handshake\": true"));
    assert!(re_emitted.contains("\"heartbeat\": \"10s\""));
}

/// Round-trip: mihomo → canonical → mihomo preserves hysteria2 hop-interval.
#[test]
fn mihomo_hysteria2_hop_interval_round_trip() {
    let yaml = format!(
        r#"
proxies:
  - name: "hy2-rt"
    type: hysteria2
    server: hy2.example.com
    port: 443
    password: "{PWD}"
    sni: hy2.example.com
    up: "100 Mbps"
    down: "200 Mbps"
    ports: "20000-40000"
    hop-interval: "30"
    obfs: salamander
    obfs-password: obfs-pwd
"#,
        PWD = common::RESERVED_PASSWORD
    );
    let nodes = deve_sub_protocol::container::parse_mihomo_yaml(&yaml).expect("parse");
    assert_eq!(nodes.len(), 1);
    let node = &nodes[0];

    let ProtocolConfig::Hysteria2(cfg) = &node.config else {
        panic!("expected Hysteria2 config");
    };
    assert_eq!(cfg.ports.as_deref(), Some("20000-40000"));
    assert_eq!(cfg.hop_interval, Some(time::Duration::seconds(30)));

    let re_emitted = emit_mihomo(std::slice::from_ref(node)).expect("emit");
    assert!(re_emitted.contains("ports: \"20000-40000\""));
    assert!(re_emitted.contains("hop-interval: 30"));
}
