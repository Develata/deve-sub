//! Property-based round-trip tests (PARSE-017).
//!
//! Property: for any valid share URI that `parse_uri` accepts,
//! `parse_uri(emit_uri(parse_uri(uri)))` must produce a node semantically
//! equal to `parse_uri(uri)`. This is the idempotency property of
//! parse ∘ emit on the set of parseable URIs.
//!
//! Strategies generate valid URI strings by construction for each of the
//! seven P0 protocols, varying host types, ports, optional fields, and
//! transport kinds. Container format round-trip is covered by the golden
//! test suites; this file focuses on URI-level round-trip.

#![allow(clippy::expect_used)]

use proptest::prelude::*;

use deve_sub_protocol::parse_uri;

use base64::Engine;

// --- Shared strategies ---

fn host_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("example.com".to_owned()),
        Just("sub.example.com".to_owned()),
        Just("[2001:db8::1]".to_owned()),
    ]
}

fn port_strategy() -> impl Strategy<Value = u16> {
    prop_oneof![Just(443), Just(8388), Just(1080), Just(2053), Just(8443),]
}

fn name_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![
        Just(None),
        Just(Some("Test".to_owned())),
        Just(Some("My Node".to_owned())),
        Just(Some("测试节点".to_owned())),
    ]
}

fn sni_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![Just(None), Just(Some("sni.example.com".to_owned())),]
}

fn alpn_strategy() -> impl Strategy<Value = Option<&'static str>> {
    prop_oneof![
        Just(None),
        Just(Some("h2")),
        Just(Some("h3")),
        Just(Some("http/1.1")),
    ]
}

fn transport_strategy() -> impl Strategy<Value = Option<&'static str>> {
    prop_oneof![
        Just(None),
        Just(Some("tcp")),
        Just(Some("ws")),
        Just(Some("grpc")),
    ]
}

/// ADR-0005 three-state: absent (None), `0` (Some(false)), `1` (Some(true)).
/// Expressed as the string value placed in the URI query.
fn insecure_strategy() -> impl Strategy<Value = Option<&'static str>> {
    prop_oneof![Just(None), Just(Some("0")), Just(Some("1")),]
}

/// Build a query string from optional parameters.
fn query(params: &[Option<(&str, String)>]) -> String {
    let parts: Vec<String> = params
        .iter()
        .flatten()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

/// Build the fragment (display name) part of a URI.
fn fragment(name: &Option<String>) -> String {
    name.as_ref()
        .map(|n| format!("#{}", url_encode(n)))
        .unwrap_or_default()
}

fn url_encode(s: &str) -> String {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

// --- Per-protocol URI strategies ---

fn vless_uri_strategy() -> impl Strategy<Value = String> {
    (
        host_strategy(),
        port_strategy(),
        sni_strategy(),
        name_strategy(),
    )
        .prop_map(|(host, port, sni, name)| {
            let mut params = vec![Some(("encryption", "none".to_owned()))];
            params.push(Some(("security", "reality".to_owned())));
            params.push(Some(("pbk", "TEST_PUBLIC_KEY".to_owned())));
            params.push(Some(("sid", "01020304".to_owned())));
            if let Some(s) = &sni {
                params.push(Some(("sni", s.clone())));
            }
            params.push(Some(("fp", "chrome".to_owned())));
            params.push(Some(("type", "tcp".to_owned())));
            format!(
                "vless://00000000-0000-4000-8000-000000000001@{host}:{port}{}{}",
                query(&params),
                fragment(&name)
            )
        })
}

fn hysteria2_uri_strategy() -> impl Strategy<Value = String> {
    (
        host_strategy(),
        port_strategy(),
        sni_strategy(),
        alpn_strategy(),
        insecure_strategy(),
        name_strategy(),
    )
        .prop_map(|(host, port, sni, alpn, insecure, name)| {
            let mut params: Vec<Option<(&str, String)>> = vec![];
            if let Some(s) = &sni {
                params.push(Some(("sni", s.clone())));
            }
            if let Some(a) = alpn {
                params.push(Some(("alpn", a.to_owned())));
            }
            if let Some(i) = insecure {
                params.push(Some(("insecure", i.to_owned())));
            }
            format!(
                "hysteria2://TEST_PASSWORD@{host}:{port}{}{}",
                query(&params),
                fragment(&name)
            )
        })
}

fn tuic_uri_strategy() -> impl Strategy<Value = String> {
    (
        host_strategy(),
        port_strategy(),
        sni_strategy(),
        alpn_strategy(),
        insecure_strategy(),
        name_strategy(),
    )
        .prop_map(|(host, port, sni, alpn, insecure, name)| {
            let mut params: Vec<Option<(&str, String)>> = vec![];
            if let Some(s) = &sni {
                params.push(Some(("sni", s.clone())));
            }
            if let Some(a) = alpn {
                params.push(Some(("alpn", a.to_owned())));
            }
            if let Some(i) = insecure {
                params.push(Some(("allowInsecure", i.to_owned())));
            }
            params.push(Some(("congestion_control", "bbr".to_owned())));
            params.push(Some(("udp_relay_mode", "native".to_owned())));
            format!(
                "tuic://00000000-0000-4000-8000-000000000001:TEST_PASSWORD@{host}:{port}{}{}",
                query(&params),
                fragment(&name)
            )
        })
}

fn naive_uri_strategy() -> impl Strategy<Value = String> {
    (host_strategy(), port_strategy(), name_strategy()).prop_map(|(host, port, name)| {
        format!(
            "naive+https://testuser:TEST_PASSWORD@{host}:{port}{}",
            fragment(&name)
        )
    })
}

fn shadowsocks_uri_strategy() -> impl Strategy<Value = String> {
    (host_strategy(), port_strategy(), name_strategy()).prop_map(|(host, port, name)| {
        // SIP002: ss://base64(method:password)@host:port#name
        let userinfo =
            base64::engine::general_purpose::STANDARD_NO_PAD.encode("aes-256-gcm:TEST_PASSWORD");
        format!("ss://{userinfo}@{host}:{port}{}", fragment(&name))
    })
}

fn trojan_uri_strategy() -> impl Strategy<Value = String> {
    (
        host_strategy(),
        port_strategy(),
        sni_strategy(),
        alpn_strategy(),
        transport_strategy(),
        insecure_strategy(),
        name_strategy(),
    )
        .prop_map(|(host, port, sni, alpn, transport, insecure, name)| {
            let mut params: Vec<Option<(&str, String)>> = vec![];
            if let Some(s) = &sni {
                params.push(Some(("sni", s.clone())));
            }
            if let Some(a) = alpn {
                params.push(Some(("alpn", a.to_owned())));
            }
            if let Some(t) = transport {
                params.push(Some(("type", t.to_owned())));
            }
            if let Some(i) = insecure {
                params.push(Some(("allowInsecure", i.to_owned())));
            }
            format!(
                "trojan://TEST_PASSWORD@{host}:{port}{}{}",
                query(&params),
                fragment(&name)
            )
        })
}

fn vmess_uri_strategy() -> impl Strategy<Value = String> {
    (
        host_strategy(),
        port_strategy(),
        name_strategy(),
        transport_strategy(),
    )
        .prop_map(|(host, port, name, transport)| {
            let net = transport.unwrap_or("tcp");
            let json = serde_json::json!({
                "v": "2",
                "ps": name.as_deref().unwrap_or(""),
                "add": host.trim_start_matches('[').trim_end_matches(']'),
                "port": port.to_string(),
                "id": "00000000-0000-4000-8000-000000000001",
                "aid": "0",
                "scy": "auto",
                "net": net,
                "type": "none",
                "host": "",
                "path": "",
                "tls": "tls",
                "sni": "",
                "alpn": ""
            });
            let encoded = base64::engine::general_purpose::STANDARD.encode(
                serde_json::to_string(&json).expect("vmess json is serializable by construction"),
            );
            format!("vmess://{encoded}")
        })
}

// --- Idempotency property ---

/// Compare two nodes on all URI-round-trippable fields.
fn assert_semantic_equal(
    n1: &deve_sub_domain::Node,
    n2: &deve_sub_domain::Node,
) -> Result<(), TestCaseError> {
    prop_assert_eq!(&n1.protocol, &n2.protocol);
    prop_assert_eq!(&n1.config, &n2.config);
    prop_assert_eq!(&n1.endpoint, &n2.endpoint);
    prop_assert_eq!(&n1.authentication, &n2.authentication);
    prop_assert_eq!(&n1.tls, &n2.tls);
    prop_assert_eq!(&n1.transport, &n2.transport);
    prop_assert_eq!(&n1.display_name, &n2.display_name);
    prop_assert_eq!(&n1.udp, &n2.udp);
    prop_assert_eq!(&n1.obfuscation, &n2.obfuscation);
    prop_assert_eq!(&n1.congestion, &n2.congestion);
    prop_assert_eq!(&n1.multiplex, &n2.multiplex);
    prop_assert_eq!(&n1.extras, &n2.extras);
    Ok(())
}

proptest! {
    #[test]
    fn vless_round_trip_idempotent(uri in vless_uri_strategy()) {
        let n1 = parse_uri(&uri)?;
        let emitted = deve_sub_emitter::emit_uri(&n1)?;
        let n2 = parse_uri(&emitted)?;
        assert_semantic_equal(&n1, &n2)?;
    }

    #[test]
    fn hysteria2_round_trip_idempotent(uri in hysteria2_uri_strategy()) {
        let n1 = parse_uri(&uri)?;
        let emitted = deve_sub_emitter::emit_uri(&n1)?;
        let n2 = parse_uri(&emitted)?;
        assert_semantic_equal(&n1, &n2)?;
    }

    #[test]
    fn tuic_round_trip_idempotent(uri in tuic_uri_strategy()) {
        let n1 = parse_uri(&uri)?;
        let emitted = deve_sub_emitter::emit_uri(&n1)?;
        let n2 = parse_uri(&emitted)?;
        assert_semantic_equal(&n1, &n2)?;
    }

    #[test]
    fn naive_round_trip_idempotent(uri in naive_uri_strategy()) {
        let n1 = parse_uri(&uri)?;
        let emitted = deve_sub_emitter::emit_uri(&n1)?;
        let n2 = parse_uri(&emitted)?;
        assert_semantic_equal(&n1, &n2)?;
    }

    #[test]
    fn shadowsocks_round_trip_idempotent(uri in shadowsocks_uri_strategy()) {
        let n1 = parse_uri(&uri)?;
        let emitted = deve_sub_emitter::emit_uri(&n1)?;
        let n2 = parse_uri(&emitted)?;
        assert_semantic_equal(&n1, &n2)?;
    }

    #[test]
    fn trojan_round_trip_idempotent(uri in trojan_uri_strategy()) {
        let n1 = parse_uri(&uri)?;
        let emitted = deve_sub_emitter::emit_uri(&n1)?;
        let n2 = parse_uri(&emitted)?;
        assert_semantic_equal(&n1, &n2)?;
    }

    #[test]
    fn vmess_round_trip_idempotent(uri in vmess_uri_strategy()) {
        let n1 = parse_uri(&uri)?;
        let emitted = deve_sub_emitter::emit_uri(&n1)?;
        let n2 = parse_uri(&emitted)?;
        assert_semantic_equal(&n1, &n2)?;
    }
}
