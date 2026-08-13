//! OUT-003: sing-box `check` validation.
//!
//! Emits a multi-protocol subscription via `emit_singbox` and pipes the
//! output through `sing-box check -c <file>`. The test is skipped when
//! the `sing-box` binary is not on PATH (CI without the tool still gets
//! fmt/clippy/test coverage from the rest of the suite).

#![allow(clippy::expect_used)]

use std::process::Command;

use deve_sub_domain::{
    Authentication, DomainName, Endpoint, Host, Hysteria2Config, Node, NodeSource, ProtocolConfig,
    ProtocolKind, RegionAssignment, RegionMethod, ShadowsocksConfig, TlsConfig, Transport,
    TransportKind, TrojanConfig, TuicV5Config, UdpCapability, UdpRelayMode, VlessRealityConfig,
};
use deve_sub_kernel::{NodeId, Timestamp};
use tempfile::NamedTempFile;

const RESERVED_UUID: &str = "00000000-0000-4000-8000-000000000001";
const RESERVED_PASSWORD: &str = "TEST_PASSWORD";

/// Returns `true` when `sing-box` is on PATH.
fn singbox_available() -> bool {
    Command::new("sing-box")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// OUT-003: emitted sing-box JSON passes `sing-box check` for every
/// supported protocol.
#[test]
fn out003_singbox_check_passes() {
    if !singbox_available() {
        eprintln!("skip: sing-box binary not on PATH");
        return;
    }

    let nodes = sample_nodes();
    let json = deve_sub_emitter::emit_singbox(&nodes).expect("emit sing-box");

    let tmp = NamedTempFile::new().expect("temp file");
    std::fs::write(tmp.path(), &json).expect("write config");

    let output = Command::new("sing-box")
        .arg("check")
        .arg("-c")
        .arg(tmp.path())
        .output()
        .expect("run sing-box check");

    assert!(
        output.status.success(),
        "sing-box check failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// OUT-003 (negative): malformed JSON is rejected by `sing-box check`,
/// proving the check is a real validation, not a no-op.
#[test]
fn out003_singbox_check_rejects_garbage() {
    if !singbox_available() {
        eprintln!("skip: sing-box binary not on PATH");
        return;
    }

    let tmp = NamedTempFile::new().expect("temp file");
    std::fs::write(tmp.path(), b"{not valid json").expect("write config");

    let output = Command::new("sing-box")
        .arg("check")
        .arg("-c")
        .arg(tmp.path())
        .output()
        .expect("run sing-box check");

    assert!(
        !output.status.success(),
        "sing-box check should reject garbage, but succeeded"
    );
}

// --- fixtures ---

fn sample_nodes() -> Vec<Node> {
    vec![
        trojan_node(),
        shadowsocks_node(),
        vless_reality_node(),
        vmess_node(),
        hysteria2_node(),
        tuic_v5_node(),
    ]
}

fn trojan_node() -> Node {
    Node {
        id: NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAA00").expect("ulid"),
        display_name: "trojan-test".to_owned(),
        protocol: ProtocolKind::Trojan,
        config: ProtocolConfig::Trojan(TrojanConfig {
            packet_encoding: None,
        }),
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new("trojan.example.com".to_owned())),
            port: 443,
        },
        authentication: Authentication::Password {
            password: RESERVED_PASSWORD.to_owned(),
        },
        transport: Some(Transport {
            kind: TransportKind::Ws,
            path: Some("/ws".to_owned()),
            host: Some("trojan.example.com".to_owned()),
            xhttp_mode: None,
        }),
        tls: Some(TlsConfig {
            enabled: true,
            server_name: Some("trojan.example.com".to_owned()),
            skip_cert_verify: None,
            alpn: vec![],
            client_fingerprint: None,
            certificate_pins: vec![],
            reality: None,
        }),
        udp: UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "test".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::from_unix_ms(0).expect("ts"),
        },
        tags: vec![],
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: None,
        },
        extras: std::collections::BTreeMap::new(),
    }
}

fn shadowsocks_node() -> Node {
    Node {
        id: NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAA01").expect("ulid"),
        display_name: "ss-test".to_owned(),
        protocol: ProtocolKind::Shadowsocks,
        config: ProtocolConfig::Shadowsocks(ShadowsocksConfig {
            method: "aes-256-gcm".to_owned(),
            plugin: None,
            plugin_opts: None,
        }),
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new("ss.example.com".to_owned())),
            port: 8388,
        },
        authentication: Authentication::Password {
            password: RESERVED_PASSWORD.to_owned(),
        },
        transport: None,
        tls: None,
        udp: UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "test".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::from_unix_ms(0).expect("ts"),
        },
        tags: vec![],
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: None,
        },
        extras: std::collections::BTreeMap::new(),
    }
}

fn vless_reality_node() -> Node {
    Node {
        id: NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAA02").expect("ulid"),
        display_name: "vless-reality-test".to_owned(),
        protocol: ProtocolKind::Vless,
        config: ProtocolConfig::VlessReality(VlessRealityConfig {
            encryption: None,
            flow: Some("xtls-rprx-vision".to_owned()),
            packet_encoding: None,
        }),
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new("vless.example.com".to_owned())),
            port: 443,
        },
        authentication: Authentication::Uuid {
            uuid: RESERVED_UUID.to_owned(),
        },
        transport: None,
        tls: Some(TlsConfig {
            enabled: true,
            server_name: Some("vless.example.com".to_owned()),
            skip_cert_verify: None,
            alpn: vec![],
            client_fingerprint: Some("chrome".to_owned()),
            certificate_pins: vec![],
            reality: Some(deve_sub_domain::RealityConfig {
                public_key: "TEST_PUBLIC_KEY".to_owned(),
                short_id: "01020304".to_owned(),
                spider_x: None,
            }),
        }),
        udp: UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "test".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::from_unix_ms(0).expect("ts"),
        },
        tags: vec![],
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: None,
        },
        extras: std::collections::BTreeMap::new(),
    }
}

fn vmess_node() -> Node {
    Node {
        id: NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAA03").expect("ulid"),
        display_name: "vmess-test".to_owned(),
        protocol: ProtocolKind::VMess,
        config: ProtocolConfig::VMess(deve_sub_domain::VMessConfig {
            alter_id: Some(0),
            security: None,
            packet_encoding: None,
        }),
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new("vmess.example.com".to_owned())),
            port: 443,
        },
        authentication: Authentication::Uuid {
            uuid: RESERVED_UUID.to_owned(),
        },
        transport: Some(Transport {
            kind: TransportKind::Ws,
            path: Some("/vmess".to_owned()),
            host: Some("vmess.example.com".to_owned()),
            xhttp_mode: None,
        }),
        tls: Some(TlsConfig {
            enabled: true,
            server_name: Some("vmess.example.com".to_owned()),
            skip_cert_verify: None,
            alpn: vec![],
            client_fingerprint: None,
            certificate_pins: vec![],
            reality: None,
        }),
        udp: UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "test".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::from_unix_ms(0).expect("ts"),
        },
        tags: vec![],
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: None,
        },
        extras: std::collections::BTreeMap::new(),
    }
}

fn hysteria2_node() -> Node {
    Node {
        id: NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAA04").expect("ulid"),
        display_name: "hy2-test".to_owned(),
        protocol: ProtocolKind::Hysteria2,
        config: ProtocolConfig::Hysteria2(Hysteria2Config {
            ports: None,
            hop_interval: None,
            fast_open: None,
            lazy: None,
        }),
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new("hy2.example.com".to_owned())),
            port: 443,
        },
        authentication: Authentication::Password {
            password: RESERVED_PASSWORD.to_owned(),
        },
        transport: None,
        tls: Some(TlsConfig {
            enabled: true,
            server_name: Some("hy2.example.com".to_owned()),
            skip_cert_verify: None,
            alpn: vec![],
            client_fingerprint: None,
            certificate_pins: vec![],
            reality: None,
        }),
        udp: UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "test".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::from_unix_ms(0).expect("ts"),
        },
        tags: vec![],
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: None,
        },
        extras: std::collections::BTreeMap::new(),
    }
}

fn tuic_v5_node() -> Node {
    Node {
        id: NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAA05").expect("ulid"),
        display_name: "tuic-test".to_owned(),
        protocol: ProtocolKind::TuicV5,
        config: ProtocolConfig::TuicV5(TuicV5Config {
            udp_relay_mode: Some(UdpRelayMode::Native),
            zero_rtt_handshake: None,
            heartbeat: None,
            disable_sni: None,
        }),
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new("tuic.example.com".to_owned())),
            port: 443,
        },
        authentication: Authentication::UuidPassword {
            uuid: RESERVED_UUID.to_owned(),
            password: RESERVED_PASSWORD.to_owned(),
        },
        transport: None,
        tls: Some(TlsConfig {
            enabled: true,
            server_name: Some("tuic.example.com".to_owned()),
            skip_cert_verify: None,
            alpn: vec!["h3".to_owned()],
            client_fingerprint: None,
            certificate_pins: vec![],
            reality: None,
        }),
        udp: UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "test".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::from_unix_ms(0).expect("ts"),
        },
        tags: vec![],
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: None,
        },
        extras: std::collections::BTreeMap::new(),
    }
}
