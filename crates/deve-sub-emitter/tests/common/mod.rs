//! Shared Node fixtures for OUT-xxx integration tests.
//!
//! Each fixture uses reserved test identifiers (constraint #9: no real
//! credentials). The protocol set covers the P0 protocols that every
//! container emitter supports.

#![allow(clippy::expect_used)]

use deve_sub_domain::{
    Authentication, DomainName, Endpoint, Host, Hysteria2Config, Node, NodeSource, ProtocolConfig,
    ProtocolKind, RealityConfig, RegionAssignment, RegionMethod, ShadowsocksConfig, TlsConfig,
    Transport, TransportKind, TrojanConfig, TuicV5Config, UdpCapability, UdpRelayMode,
    VlessRealityConfig,
};
use deve_sub_kernel::{NodeId, Timestamp};

pub const RESERVED_UUID: &str = "00000000-0000-4000-8000-000000000001";
pub const RESERVED_PASSWORD: &str = "TEST_PASSWORD";

/// Six P0 protocol nodes for multi-protocol validation.
pub fn sample_nodes() -> Vec<Node> {
    vec![
        trojan_node(),
        shadowsocks_node(),
        vless_reality_node(),
        vmess_node(),
        hysteria2_node(),
        tuic_v5_node(),
    ]
}

/// Filter `sample_nodes()` to only those compatible with `profile`.
///
/// WHY: xray/v2ray do not support Hysteria2 or TUIC v5; the compatibility
/// layer must exclude them before the emitter sees them (constraint #7).
#[allow(dead_code)]
pub fn compatible_nodes(profile: deve_sub_compatibility::ProfileKind) -> Vec<Node> {
    let cap = deve_sub_compatibility::capability_for(profile);
    sample_nodes()
        .into_iter()
        .filter(|n| deve_sub_compatibility::check_node(n, &cap).is_ok())
        .collect()
}

fn base_node(id: &str, name: &str, protocol: ProtocolKind, config: ProtocolConfig) -> Node {
    Node {
        id: NodeId::parse(id).expect("ulid"),
        display_name: name.to_owned(),
        protocol,
        config,
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new("example.com".to_owned())),
            port: 443,
        },
        authentication: Authentication::None,
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

fn trojan_node() -> Node {
    let mut n = base_node(
        "01KZAAAAAAAAAAAAAAAAAAAA00",
        "trojan-test",
        ProtocolKind::Trojan,
        ProtocolConfig::Trojan(TrojanConfig {
            packet_encoding: None,
        }),
    );
    n.endpoint.host = Host::Domain(DomainName::new("trojan.example.com".to_owned()));
    n.authentication = Authentication::Password {
        password: RESERVED_PASSWORD.to_owned(),
    };
    n.transport = Some(Transport {
        kind: TransportKind::Ws,
        path: Some("/ws".to_owned()),
        host: Some("trojan.example.com".to_owned()),
        xhttp_mode: None,
    });
    n.tls = Some(TlsConfig {
        enabled: true,
        server_name: Some("trojan.example.com".to_owned()),
        skip_cert_verify: None,
        alpn: vec![],
        client_fingerprint: None,
        certificate_pins: vec![],
        reality: None,
    });
    n
}

fn shadowsocks_node() -> Node {
    let mut n = base_node(
        "01KZAAAAAAAAAAAAAAAAAAAA01",
        "ss-test",
        ProtocolKind::Shadowsocks,
        ProtocolConfig::Shadowsocks(ShadowsocksConfig {
            method: "aes-256-gcm".to_owned(),
            plugin: None,
            plugin_opts: None,
        }),
    );
    n.endpoint.host = Host::Domain(DomainName::new("ss.example.com".to_owned()));
    n.endpoint.port = 8388;
    n.authentication = Authentication::Password {
        password: RESERVED_PASSWORD.to_owned(),
    };
    n
}

fn vless_reality_node() -> Node {
    let mut n = base_node(
        "01KZAAAAAAAAAAAAAAAAAAAA02",
        "vless-reality-test",
        ProtocolKind::Vless,
        ProtocolConfig::VlessReality(VlessRealityConfig {
            encryption: None,
            flow: Some("xtls-rprx-vision".to_owned()),
            packet_encoding: None,
        }),
    );
    n.endpoint.host = Host::Domain(DomainName::new("vless.example.com".to_owned()));
    n.authentication = Authentication::Uuid {
        uuid: RESERVED_UUID.to_owned(),
    };
    n.tls = Some(TlsConfig {
        enabled: true,
        server_name: Some("vless.example.com".to_owned()),
        skip_cert_verify: None,
        alpn: vec![],
        client_fingerprint: Some("chrome".to_owned()),
        certificate_pins: vec![],
        reality: Some(RealityConfig {
            public_key: "TEST_PUBLIC_KEY".to_owned(),
            short_id: "01020304".to_owned(),
            spider_x: None,
        }),
    });
    n
}

fn vmess_node() -> Node {
    let mut n = base_node(
        "01KZAAAAAAAAAAAAAAAAAAAA03",
        "vmess-test",
        ProtocolKind::VMess,
        ProtocolConfig::VMess(deve_sub_domain::VMessConfig {
            alter_id: Some(0),
            security: None,
            packet_encoding: None,
        }),
    );
    n.endpoint.host = Host::Domain(DomainName::new("vmess.example.com".to_owned()));
    n.authentication = Authentication::Uuid {
        uuid: RESERVED_UUID.to_owned(),
    };
    n.transport = Some(Transport {
        kind: TransportKind::Ws,
        path: Some("/vmess".to_owned()),
        host: Some("vmess.example.com".to_owned()),
        xhttp_mode: None,
    });
    n.tls = Some(TlsConfig {
        enabled: true,
        server_name: Some("vmess.example.com".to_owned()),
        skip_cert_verify: None,
        alpn: vec![],
        client_fingerprint: None,
        certificate_pins: vec![],
        reality: None,
    });
    n
}

fn hysteria2_node() -> Node {
    let mut n = base_node(
        "01KZAAAAAAAAAAAAAAAAAAAA04",
        "hy2-test",
        ProtocolKind::Hysteria2,
        ProtocolConfig::Hysteria2(Hysteria2Config {
            ports: None,
            hop_interval: None,
            fast_open: None,
            lazy: None,
        }),
    );
    n.endpoint.host = Host::Domain(DomainName::new("hy2.example.com".to_owned()));
    n.authentication = Authentication::Password {
        password: RESERVED_PASSWORD.to_owned(),
    };
    n.tls = Some(TlsConfig {
        enabled: true,
        server_name: Some("hy2.example.com".to_owned()),
        skip_cert_verify: None,
        alpn: vec![],
        client_fingerprint: None,
        certificate_pins: vec![],
        reality: None,
    });
    n
}

fn tuic_v5_node() -> Node {
    let mut n = base_node(
        "01KZAAAAAAAAAAAAAAAAAAAA05",
        "tuic-test",
        ProtocolKind::TuicV5,
        ProtocolConfig::TuicV5(TuicV5Config {
            udp_relay_mode: Some(UdpRelayMode::Native),
            zero_rtt_handshake: None,
            heartbeat: None,
            disable_sni: None,
        }),
    );
    n.endpoint.host = Host::Domain(DomainName::new("tuic.example.com".to_owned()));
    n.authentication = Authentication::UuidPassword {
        uuid: RESERVED_UUID.to_owned(),
        password: RESERVED_PASSWORD.to_owned(),
    };
    n.tls = Some(TlsConfig {
        enabled: true,
        server_name: Some("tuic.example.com".to_owned()),
        skip_cert_verify: None,
        alpn: vec!["h3".to_owned()],
        client_fingerprint: None,
        certificate_pins: vec![],
        reality: None,
    });
    n
}
