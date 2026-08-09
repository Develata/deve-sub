//! VLESS protocol client for the real-proxy probe.
//!
//! Wire format (request): version(0x00) + UUID(16) + addons_len(0) +
//! cmd(0x01=TCP) + port(2 BE) + ATYP + addr. No padding in basic VLESS.
//! Response: version(0x00) + addons_len + addons — consumed here so the
//! returned stream starts at the tunneled HTTP response.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;

use deve_sub_domain::{Authentication, ErrorClass, Node};

use super::stream::BoxedStream;
use super::target::TestTarget;
use super::tls::skip_verify_connector;

pub async fn dial(
    node: &Node,
    target: &TestTarget,
    timeout: Duration,
) -> Result<BoxedStream, ErrorClass> {
    tokio::time::timeout(timeout, dial_inner(node, target))
        .await
        .map_err(|_| ErrorClass::Timeout)?
}

async fn dial_inner(node: &Node, target: &TestTarget) -> Result<BoxedStream, ErrorClass> {
    let uuid_str = match &node.authentication {
        Authentication::Uuid { uuid } => uuid,
        _ => return Err(ErrorClass::Refused),
    };
    let uuid = Uuid::parse_str(uuid_str).map_err(|_| ErrorClass::Refused)?;

    if node.tls.as_ref().and_then(|t| t.reality.as_ref()).is_some() {
        tracing::debug!(
            protocol = "vless-reality",
            "Reality TLS handshake not supported by probe; returning Refused"
        );
        return Err(ErrorClass::Refused);
    }

    let tcp = TcpStream::connect((node.endpoint.host.uri_host(), node.endpoint.port))
        .await
        .map_err(|_| ErrorClass::Refused)?;

    let sni = node
        .tls
        .as_ref()
        .and_then(|t| t.server_name.clone())
        .unwrap_or_else(|| node.endpoint.host.uri_host());

    let connector = skip_verify_connector(vec![]).map_err(|_| ErrorClass::Refused)?;
    let server_name =
        rustls::pki_types::ServerName::try_from(sni).map_err(|_| ErrorClass::Refused)?;
    let mut tls = connector
        .connect(server_name, tcp)
        .await
        .map_err(|_| ErrorClass::Refused)?;

    let mut header = Vec::with_capacity(1 + 16 + 1 + 1 + 2 + 1 + 255);
    header.push(0x00); // version
    header.extend_from_slice(uuid.as_bytes());
    header.push(0x00); // addons length
    header.push(0x01); // command: TCP
    header.extend_from_slice(&target.port().to_be_bytes());
    header.extend_from_slice(&vless_addr(target));
    tls.write_all(&header)
        .await
        .map_err(|_| ErrorClass::Refused)?;

    // Consume VLESS response header: version(1) + addons_len(1) + addons.
    let mut resp_hdr = [0u8; 2];
    tls.read_exact(&mut resp_hdr)
        .await
        .map_err(|_| ErrorClass::Refused)?;
    let addons_len = resp_hdr[1] as usize;
    if addons_len > 0 {
        let mut addons = vec![0u8; addons_len];
        tls.read_exact(&mut addons)
            .await
            .map_err(|_| ErrorClass::Refused)?;
    }

    Ok(Box::new(tls))
}

fn vless_addr(target: &TestTarget) -> Vec<u8> {
    let host = target.host();
    let mut buf = Vec::with_capacity(1 + 255);
    if let Ok(ipv4) = host.parse::<Ipv4Addr>() {
        buf.push(0x01);
        buf.extend_from_slice(&ipv4.octets());
    } else if let Ok(ipv6) = host.parse::<Ipv6Addr>() {
        buf.push(0x03);
        buf.extend_from_slice(&ipv6.octets());
    } else {
        buf.push(0x02);
        buf.push(host.len() as u8);
        buf.extend_from_slice(host.as_bytes());
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::real_proxy::RealProxyProbe;
    use crate::real_proxy::test_util::{LocalHttpTarget, TestCert};
    use deve_sub_domain::{
        Authentication, Endpoint, Host, LatencyProbe, Node, NodeSource, ProtocolConfig,
        ProtocolKind, RealityConfig, RegionAssignment, RegionMethod, TlsConfig, UdpCapability,
        VlessRealityConfig,
    };
    use deve_sub_kernel::{NodeId, Timestamp};
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[tokio::test]
    async fn vless_round_trip() {
        let http_target = LocalHttpTarget::start().await;
        let cert = TestCert::generate();
        let acceptor = cert.acceptor();

        let vless_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let vless_addr = vless_listener.local_addr().expect("addr");

        let target_port = http_target.addr().port();
        let server = tokio::spawn(async move {
            let (tcp, _) = vless_listener.accept().await.expect("accept");
            let mut tls = acceptor.accept(tcp).await.expect("tls accept");

            // Read VLESS request header.
            let mut version = [0u8; 1];
            tls.read_exact(&mut version).await.expect("version");
            let mut uuid_bytes = [0u8; 16];
            tls.read_exact(&mut uuid_bytes).await.expect("uuid");
            let mut addons_len = [0u8; 1];
            tls.read_exact(&mut addons_len).await.expect("addons_len");
            if addons_len[0] > 0 {
                let mut addons = vec![0u8; addons_len[0] as usize];
                tls.read_exact(&mut addons).await.expect("addons");
            }
            let mut cmd = [0u8; 1];
            tls.read_exact(&mut cmd).await.expect("cmd");
            let mut port = [0u8; 2];
            tls.read_exact(&mut port).await.expect("port");
            let mut atyp = [0u8; 1];
            tls.read_exact(&mut atyp).await.expect("atyp");
            let target_sock = match atyp[0] {
                0x01 => {
                    let mut addr = [0u8; 4];
                    tls.read_exact(&mut addr).await.expect("ipv4");
                    std::net::SocketAddr::V4(std::net::SocketAddrV4::new(
                        Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3]),
                        u16::from_be_bytes(port),
                    ))
                }
                _ => panic!("unexpected atyp: {}", atyp[0]),
            };

            // Send VLESS response header.
            tls.write_all(&[0x00, 0x00]).await.expect("resp hdr");

            // Relay to target.
            let mut target_stream = tokio::net::TcpStream::connect(target_sock)
                .await
                .expect("connect");
            let _ = tokio::io::copy_bidirectional(&mut tls, &mut target_stream).await;
        });

        let node = build_vless_node(vless_addr);
        let target = TestTarget::new("127.0.0.1", target_port, "/");
        let probe = RealProxyProbe::with_target(target);
        let result = probe.probe(&node, Duration::from_secs(5)).await;

        assert_eq!(result.error_class, ErrorClass::Ok);
        assert!(result.rtt_ms.is_some(), "should have RTT");

        http_target.abort();
        server.abort();
    }

    fn build_vless_node(addr: std::net::SocketAddr) -> Node {
        Node {
            id: NodeId::new(),
            display_name: "test-vless".to_owned(),
            protocol: ProtocolKind::Vless,
            config: ProtocolConfig::VlessReality(VlessRealityConfig {
                encryption: Some("none".to_owned()),
                flow: None,
                packet_encoding: None,
            }),
            endpoint: Endpoint {
                host: Host::Ipv4("127.0.0.1".parse().expect("ipv4")),
                port: addr.port(),
            },
            authentication: Authentication::Uuid {
                uuid: "12345678-1234-1234-1234-123456789abc".to_owned(),
            },
            transport: None,
            tls: Some(TlsConfig {
                enabled: true,
                server_name: Some("127.0.0.1".to_owned()),
                skip_cert_verify: Some(true),
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
                imported_at: Timestamp::now(),
            },
            tags: vec![],
            region: RegionAssignment {
                method: RegionMethod::Auto,
                value: None,
            },
            extras: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn vless_reality_rejected() {
        let http_target = LocalHttpTarget::start().await;
        let target_port = http_target.addr().port();

        let mut node = build_vless_node("127.0.0.1:0".parse().expect("addr"));
        if let Some(tls) = node.tls.as_mut() {
            tls.reality = Some(RealityConfig {
                public_key: "test-pbk".to_owned(),
                short_id: "01".to_owned(),
                spider_x: None,
            });
        }

        let target = TestTarget::new("127.0.0.1", target_port, "/");
        let probe = RealProxyProbe::with_target(target);
        let result = probe.probe(&node, Duration::from_secs(5)).await;

        assert_eq!(
            result.error_class,
            ErrorClass::Refused,
            "VLESS Reality must be refused, not attempted"
        );
        assert!(result.rtt_ms.is_none(), "no RTT on refusal");

        http_target.abort();
    }
}
