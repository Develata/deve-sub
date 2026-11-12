#![allow(clippy::expect_used)]
//! CA01: host-only resolver inputs and actual local dual-stack probe connections.

use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use deve_sub_adapters::{QuicHandshakeProbe, TcpConnectProbe};
use deve_sub_application::source::{GeoIpPort, RegionDetection, enrich_regions, parse_for_import};
use deve_sub_domain::{
    ErrorClass, ItemParseStatus, LatencyProbe, Node, ReconcileEntry, SourceType,
};

fn node_for(host: &str, port: u16) -> Node {
    parse_for_import(
        SourceType::UriList,
        None,
        format!("trojan://TEST_ONLY@{host}:{port}#local").as_bytes(),
    )
    .expect("parse local fixture")
    .nodes
    .remove(0)
}

struct LiteralGeoIp {
    seen: Mutex<Vec<String>>,
}

#[async_trait]
impl GeoIpPort for LiteralGeoIp {
    async fn detect_region(&self, host: &str) -> RegionDetection {
        self.seen.lock().expect("seen").push(host.to_owned());
        // Like the real GeoIP adapter, IP literals are recognized before DNS.
        let candidate_ips: Vec<IpAddr> = host.parse().into_iter().collect();
        RegionDetection {
            region: (!candidate_ips.is_empty()).then(|| "TEST".into()),
            candidate_ips,
        }
    }
}

#[tokio::test]
async fn ipv6_geoip_receives_bare_literal_and_deduplicates_hosts() {
    let geoip = LiteralGeoIp {
        seen: Mutex::new(Vec::new()),
    };
    let mut entries: Vec<_> = [443, 8443]
        .into_iter()
        .map(|port| ReconcileEntry {
            raw_uri: String::new(),
            initial_status: ItemParseStatus::Parsed,
            node: Some(node_for("[2001:db8::1]", port)),
        })
        .collect();
    enrich_regions(&mut entries, &geoip).await;
    assert_eq!(*geoip.seen.lock().expect("seen"), ["2001:db8::1"]);
    for entry in entries {
        let node = entry.node.expect("node");
        assert_eq!(node.region.value.as_deref(), Some("TEST"));
        assert_eq!(
            node.extras["candidate_ips"],
            serde_json::json!(["2001:db8::1"])
        );
    }
}

#[tokio::test]
async fn ipv6_tcp_probe_connects_to_owned_listener() {
    for host in ["127.0.0.1", "[::1]"] {
        let listener = tokio::net::TcpListener::bind(format!("{host}:0"))
            .await
            .expect("local TCP listener");
        let node = node_for(host, listener.local_addr().expect("address").port());
        let result = TcpConnectProbe::new()
            .probe(&node, Duration::from_secs(2))
            .await;
        assert_eq!(result.error_class, ErrorClass::Ok, "{host}");
        assert!(result.rtt_ms.is_some());
        tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("connection reached listener")
            .expect("accept");
    }
}

#[tokio::test]
async fn ipv6_quic_probe_completes_handshake_with_owned_peer() {
    let probe = QuicHandshakeProbe::new();
    // Reuse the same probe across families, including returning to IPv4.
    for host in ["127.0.0.1", "[::1]", "127.0.0.1"] {
        let cert =
            rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("test certificate");
        let server_config = quinn::ServerConfig::with_single_cert(
            vec![cert.cert.der().clone()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into()),
        )
        .expect("QUIC config");
        let endpoint = Arc::new(
            quinn::Endpoint::server(server_config, format!("{host}:0").parse().expect("address"))
                .expect("QUIC listener"),
        );
        let mut node = node_for(host, endpoint.local_addr().expect("address").port());
        node.tls.as_mut().expect("tls").server_name = Some("localhost".into());
        let peer = Arc::clone(&endpoint);
        let server = tokio::spawn(async move {
            let connection = peer
                .accept()
                .await
                .expect("accept")
                .await
                .expect("handshake");
            // Keep the endpoint alive until the probe has received the handshake.
            connection.closed().await;
        });
        let result = probe.probe(&node, Duration::from_secs(2)).await;
        assert_eq!(result.error_class, ErrorClass::Ok, "{host}");
        assert!(result.rtt_ms.is_some());
        server.abort();
        endpoint.close(0_u32.into(), b"test complete");
    }
}
