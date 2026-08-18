//! W-Q/W-R/W-S regression tests: verify mihomo and sing-box emitters
//! preserve Reality fields, client-fingerprint, and transport Host header
//! that were previously silently dropped.

#![allow(clippy::expect_used)]

mod common;

use deve_sub_domain::{Node, Transport, TransportKind};
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
