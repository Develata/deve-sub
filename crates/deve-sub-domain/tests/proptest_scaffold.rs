//! Proptest scaffold: serde invariants for the canonical node model.
//!
//! These properties guard against regressions in three-state TLS semantics
//! (ADR-0005) and `ProtocolKind` round-trip fidelity (ADR-0003). Full
//! parse → emit property tests arrive with M3 round-trip coverage.

use proptest::prelude::*;

use deve_sub_domain::{ProtocolKind, TlsConfig};

fn skip_cert_verify_strategy() -> impl Strategy<Value = Option<bool>> {
    prop_oneof![Just(None), Just(Some(false)), Just(Some(true))]
}

fn protocol_kind_strategy() -> impl Strategy<Value = ProtocolKind> {
    prop_oneof![
        Just(ProtocolKind::Vless),
        Just(ProtocolKind::VMess),
        Just(ProtocolKind::Trojan),
        Just(ProtocolKind::Shadowsocks),
        Just(ProtocolKind::Hysteria2),
        Just(ProtocolKind::TuicV5),
        Just(ProtocolKind::NaiveProxy),
        Just(ProtocolKind::Socks5),
        Just(ProtocolKind::Http),
        Just(ProtocolKind::HysteriaV1),
        Just(ProtocolKind::AnyTls),
        Just(ProtocolKind::Snell),
        Just(ProtocolKind::WireGuard),
        Just(ProtocolKind::ShadowTls),
        Just(ProtocolKind::Ssh),
        "[a-z]{1,8}".prop_map(ProtocolKind::Unknown),
    ]
}

fn tls_strategy() -> impl Strategy<Value = TlsConfig> {
    skip_cert_verify_strategy().prop_map(|scv| TlsConfig {
        enabled: true,
        server_name: None,
        skip_cert_verify: scv,
        alpn: vec![],
        client_fingerprint: None,
        certificate_pins: vec![],
        reality: None,
    })
}

proptest! {
    #[test]
    fn protocol_kind_serde_roundtrip(kind in protocol_kind_strategy()) {
        let json = serde_json::to_string(&kind).expect("serialize");
        let recovered: ProtocolKind = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(kind, recovered);
    }

    #[test]
    fn tls_three_state_serde_roundtrip(tls in tls_strategy()) {
        let json = serde_json::to_string(&tls).expect("serialize");
        let recovered: TlsConfig = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(tls, recovered);
    }

    #[test]
    fn tls_skip_cert_verify_never_coerces_to_some_true(scv in skip_cert_verify_strategy()) {
        let tls = TlsConfig {
            enabled: true,
            server_name: None,
            skip_cert_verify: scv,
            alpn: vec![],
            client_fingerprint: None,
            certificate_pins: vec![],
            reality: None,
        };
        let json = serde_json::to_string(&tls).expect("serialize");
        let recovered: TlsConfig = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(scv, recovered.skip_cert_verify);
    }
}
