//! Trojan protocol client for the real-proxy probe.
//!
//! Wire format: `hex(SHA224(password))` + CRLF + SOCKS5 address + CRLF +
//! payload. The server forwards the payload to the addressed target and
//! relays the response back. See the Trojan protocol specification.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use sha2::{Digest, Sha224};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use deve_sub_domain::{Authentication, ErrorClass, Node};

use super::stream::BoxedStream;
use super::target::TestTarget;
use super::tls::skip_verify_connector;

/// Dial through a Trojan proxy node to `target`, returning a tunneled
/// stream. The caller sends the HTTP probe request over this stream.
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
    let password = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(ErrorClass::Refused),
    };

    let mut hasher = Sha224::new();
    hasher.update(password.as_bytes());
    let hash = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hash_hex = String::with_capacity(56);
    for &b in hash.iter() {
        hash_hex.push(HEX[(b >> 4) as usize] as char);
        hash_hex.push(HEX[(b & 0x0f) as usize] as char);
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

    let mut handshake = Vec::with_capacity(64 + 4 + 255 + 2);
    handshake.extend_from_slice(hash_hex.as_bytes());
    handshake.extend_from_slice(b"\r\n");
    handshake.extend_from_slice(&socks5_addr(target));
    handshake.extend_from_slice(b"\r\n");

    tls.write_all(&handshake)
        .await
        .map_err(|_| ErrorClass::Refused)?;

    Ok(Box::new(tls))
}

/// Build SOCKS5 address bytes: ATYP + ADDR + PORT.
fn socks5_addr(target: &TestTarget) -> Vec<u8> {
    let host = target.host();
    let port = target.port();
    let mut buf = Vec::with_capacity(1 + 255 + 2);
    if let Ok(ipv4) = host.parse::<Ipv4Addr>() {
        buf.push(0x01);
        buf.extend_from_slice(&ipv4.octets());
    } else if let Ok(ipv6) = host.parse::<Ipv6Addr>() {
        buf.push(0x04);
        buf.extend_from_slice(&ipv6.octets());
    } else {
        buf.push(0x03);
        buf.push(host.len() as u8);
        buf.extend_from_slice(host.as_bytes());
    }
    buf.extend_from_slice(&port.to_be_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::real_proxy::RealProxyProbe;
    use crate::real_proxy::test_util::{LocalHttpTarget, TestCert};
    use deve_sub_domain::{
        Authentication, Endpoint, Host, LatencyProbe, Node, NodeSource, ProtocolConfig,
        ProtocolKind, RegionAssignment, RegionMethod, TlsConfig, TrojanConfig, UdpCapability,
    };
    use deve_sub_kernel::{NodeId, Timestamp};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn trojan_round_trip() {
        let http_target = LocalHttpTarget::start().await;
        let cert = TestCert::generate();
        let acceptor = cert.acceptor();

        let trojan_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let trojan_addr = trojan_listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            let (tcp, _) = trojan_listener.accept().await.expect("accept");
            let mut tls = acceptor.accept(tcp).await.expect("tls accept");

            // Read SHA224 hex + CRLF (56 + 2 = 58 bytes).
            let mut handshake = [0u8; 58];
            tls.read_exact(&mut handshake).await.expect("read hash");

            // Read SOCKS5 address: ATYP + ADDR + PORT.
            let mut atyp = [0u8; 1];
            tls.read_exact(&mut atyp).await.expect("read atyp");
            let parsed_addr = match atyp[0] {
                0x01 => {
                    let mut addr = [0u8; 6];
                    tls.read_exact(&mut addr).await.expect("read ipv4+port");
                    let ip = Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3]);
                    let port = u16::from_be_bytes([addr[4], addr[5]]);
                    std::net::SocketAddr::V4(std::net::SocketAddrV4::new(ip, port))
                }
                _ => panic!("unexpected atyp: {}", atyp[0]),
            };

            // Read trailing CRLF.
            let mut crlf = [0u8; 2];
            tls.read_exact(&mut crlf).await.expect("read crlf");

            // Connect to parsed target and relay bidirectionally.
            let mut target_stream = tokio::net::TcpStream::connect(parsed_addr)
                .await
                .expect("connect target");
            let _ = tokio::io::copy_bidirectional(&mut tls, &mut target_stream).await;
        });

        let node = build_trojan_node(trojan_addr);
        let target = TestTarget::new("127.0.0.1", http_target.addr().port(), "/");
        let probe = RealProxyProbe::with_target(target);
        let result = probe.probe(&node, Duration::from_secs(5)).await;

        assert_eq!(result.error_class, ErrorClass::Ok);
        assert!(result.rtt_ms.is_some(), "should have RTT");

        http_target.abort();
        server.abort();
    }

    fn build_trojan_node(addr: std::net::SocketAddr) -> Node {
        Node {
            id: NodeId::new(),
            display_name: "test-trojan".to_owned(),
            protocol: ProtocolKind::Trojan,
            config: ProtocolConfig::Trojan(TrojanConfig {
                packet_encoding: None,
            }),
            endpoint: Endpoint {
                host: Host::Ipv4("127.0.0.1".parse().expect("ipv4")),
                port: addr.port(),
            },
            authentication: Authentication::Password {
                password: "test-password".to_owned(),
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
}
