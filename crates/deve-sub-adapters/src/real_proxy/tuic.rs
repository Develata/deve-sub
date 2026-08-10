//! TUIC v5 protocol client for the real-proxy probe.
//!
//! Wire format (per tuic-protocol/tuic SPEC.md):
//! 1. Auth: open a QUIC **unidirectional** stream, send
//!    `[0x05, 0x00, UUID(16 bytes), TOKEN(32 bytes)]`.
//!    TOKEN = TLS keying material exporter (RFC 5705):
//!    `conn.export_keying_material(&mut token, label=uuid_bytes, context=password_bytes)`.
//! 2. Connect: open a QUIC **bidirectional** stream, send
//!    `[0x05, 0x01, ATYP, ADDR, PORT(2 BE)]`.
//!    ATYP: 0x00=Domain(1-byte len + bytes), 0x01=IPv4(4), 0x02=IPv6(16).
//!    No server response — the stream immediately relays raw TCP.

use std::time::Duration;

use quinn::Connection;

use deve_sub_domain::{Authentication, ErrorClass, Node};

use super::quic::{QuicBidiStream, quic_connect};
use super::stream::BoxedStream;
use super::target::TestTarget;

const VER: u8 = 0x05;
const CMD_AUTH: u8 = 0x00;
const CMD_CONNECT: u8 = 0x01;

const ATYP_DOMAIN: u8 = 0x00;
const ATYP_IPV4: u8 = 0x01;
const ATYP_IPV6: u8 = 0x02;

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
    let (uuid_str, password) = match &node.authentication {
        Authentication::UuidPassword { uuid, password } => (uuid, password),
        _ => return Err(ErrorClass::Refused),
    };

    let uuid_bytes = parse_uuid(uuid_str).ok_or(ErrorClass::Refused)?;

    let (endpoint, conn) = quic_connect(node).await?;
    authenticate(&conn, &uuid_bytes, password).await?;
    let (send, recv) = open_connect_stream(&conn, target).await?;
    Ok(Box::new(QuicBidiStream {
        inner: tokio::io::join(recv, send),
        _endpoint: endpoint,
        _conn: conn,
    }))
}

async fn authenticate(
    conn: &Connection,
    uuid_bytes: &[u8; 16],
    password: &str,
) -> Result<(), ErrorClass> {
    let mut token = [0u8; 32];
    conn.export_keying_material(&mut token, uuid_bytes, password.as_bytes())
        .map_err(|_| ErrorClass::QuicFailed)?;

    let mut frame = Vec::with_capacity(2 + 16 + 32);
    frame.push(VER);
    frame.push(CMD_AUTH);
    frame.extend_from_slice(uuid_bytes);
    frame.extend_from_slice(&token);

    let mut uni = conn.open_uni().await.map_err(|_| ErrorClass::Refused)?;
    uni.write_all(&frame)
        .await
        .map_err(|_| ErrorClass::Refused)?;
    uni.finish().map_err(|_| ErrorClass::Refused)?;
    Ok(())
}

async fn open_connect_stream(
    conn: &Connection,
    target: &TestTarget,
) -> Result<(quinn::SendStream, quinn::RecvStream), ErrorClass> {
    let (mut send, recv) = conn.open_bi().await.map_err(|_| ErrorClass::Refused)?;

    let mut frame = Vec::with_capacity(2 + 1 + 255 + 2);
    frame.push(VER);
    frame.push(CMD_CONNECT);
    encode_address(&mut frame, target.host(), target.port());

    send.write_all(&frame)
        .await
        .map_err(|_| ErrorClass::Refused)?;
    Ok((send, recv))
}

fn encode_address(buf: &mut Vec<u8>, host: &str, port: u16) {
    if let Ok(ipv4) = host.parse::<std::net::Ipv4Addr>() {
        buf.push(ATYP_IPV4);
        buf.extend_from_slice(&ipv4.octets());
    } else if let Ok(ipv6) = host.parse::<std::net::Ipv6Addr>() {
        buf.push(ATYP_IPV6);
        buf.extend_from_slice(&ipv6.octets());
    } else {
        buf.push(ATYP_DOMAIN);
        buf.push(host.len() as u8);
        buf.extend_from_slice(host.as_bytes());
    }
    buf.extend_from_slice(&port.to_be_bytes());
}

fn parse_uuid(s: &str) -> Option<[u8; 16]> {
    let parsed = uuid::Uuid::parse_str(s).ok()?;
    Some(*parsed.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::real_proxy::RealProxyProbe;
    use crate::real_proxy::test_util::{LocalHttpTarget, TestCert};
    use deve_sub_domain::{
        Authentication, Endpoint, Host, LatencyProbe, Node, NodeSource, ProtocolConfig,
        ProtocolKind, RegionAssignment, RegionMethod, TlsConfig, TuicV5Config, UdpCapability,
    };
    use deve_sub_kernel::{NodeId, Timestamp};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn tuic_v5_round_trip() {
        let http_target = LocalHttpTarget::start().await;
        let target_port = http_target.addr().port();

        let cert = TestCert::generate();
        let cert_der = rustls::pki_types::CertificateDer::from(cert.cert_der.clone());
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(cert.key_der.clone().into());
        let mut tls_srv = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key)
            .expect("server config");
        tls_srv.alpn_protocols = vec![b"h3".to_vec()];
        let qsc = quinn::crypto::rustls::QuicServerConfig::try_from(tls_srv).expect("quic server");
        let server_cfg = quinn::ServerConfig::with_crypto(Arc::new(qsc));
        let srv_ep = quinn::Endpoint::server(server_cfg, "127.0.0.1:0".parse().expect("parse"))
            .expect("server endpoint");
        let tuic_addr = srv_ep.local_addr().expect("addr");

        let uuid_str = "12345678-1234-1234-1234-123456789abc".to_owned();
        let password = "test-password".to_owned();
        let srv_uuid = uuid_str.clone();
        let srv_password = password.clone();

        let server = tokio::spawn(async move {
            let conn = srv_ep.accept().await.expect("accept").await.expect("conn");

            let uuid_parsed = uuid::Uuid::parse_str(&srv_uuid).expect("uuid");
            let uuid_bytes = *uuid_parsed.as_bytes();
            let mut expected_token = [0u8; 32];
            conn.export_keying_material(&mut expected_token, &uuid_bytes, srv_password.as_bytes())
                .expect("export");

            let mut uni = conn.accept_uni().await.expect("accept_uni");
            let mut auth_buf = [0u8; 50];
            uni.read_exact(&mut auth_buf).await.expect("read auth");
            assert_eq!(auth_buf[0], VER);
            assert_eq!(auth_buf[1], CMD_AUTH);
            assert_eq!(&auth_buf[2..18], &uuid_bytes);
            assert_eq!(&auth_buf[18..50], &expected_token);

            let (send, mut recv) = conn.accept_bi().await.expect("accept_bi");

            let mut header = [0u8; 2];
            recv.read_exact(&mut header).await.expect("read header");
            assert_eq!(header[0], VER);
            assert_eq!(header[1], CMD_CONNECT);

            let atyp = {
                let mut buf = [0u8; 1];
                recv.read_exact(&mut buf).await.expect("read atyp");
                buf[0]
            };
            assert_eq!(atyp, ATYP_IPV4);

            let mut addr = [0u8; 6];
            recv.read_exact(&mut addr).await.expect("read addr+port");
            let ip = std::net::Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3]);
            let port = u16::from_be_bytes([addr[4], addr[5]]);
            let target_addr = std::net::SocketAddr::V4(std::net::SocketAddrV4::new(ip, port));

            let mut target = tokio::net::TcpStream::connect(target_addr)
                .await
                .expect("connect");
            let mut combined = tokio::io::join(recv, send);
            let _ = tokio::io::copy_bidirectional(&mut combined, &mut target).await;
        });

        let node = build_tuic_node(tuic_addr, &uuid_str, &password);
        let target = TestTarget::new("127.0.0.1", target_port, "/");
        let probe = RealProxyProbe::with_target(target);
        let result = probe.probe(&node, Duration::from_secs(10)).await;

        assert_eq!(result.error_class, ErrorClass::Ok);
        assert!(result.rtt_ms.is_some(), "should have RTT");

        http_target.abort();
        server.abort();
    }

    #[test]
    fn uuid_parsing() {
        let bytes = parse_uuid("12345678-1234-1234-1234-123456789abc");
        assert!(bytes.is_some());
        assert_eq!(bytes.expect("parsed uuid")[0], 0x12);
    }

    #[test]
    fn address_encoding_ipv4() {
        let mut buf = Vec::new();
        encode_address(&mut buf, "127.0.0.1", 80);
        assert_eq!(buf, vec![ATYP_IPV4, 127, 0, 0, 1, 0, 80]);
    }

    #[test]
    fn address_encoding_domain() {
        let mut buf = Vec::new();
        encode_address(&mut buf, "example.com", 443);
        assert_eq!(buf[0], ATYP_DOMAIN);
        assert_eq!(buf[1], 11);
        assert_eq!(&buf[2..13], b"example.com");
        assert_eq!(&buf[13..], [0x01, 0xBB]);
    }

    #[allow(clippy::expect_used, reason = "test code")]
    fn build_tuic_node(addr: std::net::SocketAddr, uuid: &str, password: &str) -> Node {
        Node {
            id: NodeId::new(),
            display_name: "test-tuic".to_owned(),
            protocol: ProtocolKind::TuicV5,
            config: ProtocolConfig::TuicV5(TuicV5Config {
                udp_relay_mode: None,
                zero_rtt_handshake: None,
                heartbeat: None,
                disable_sni: None,
            }),
            endpoint: Endpoint {
                host: Host::Ipv4("127.0.0.1".parse().expect("ipv4")),
                port: addr.port(),
            },
            authentication: Authentication::UuidPassword {
                uuid: uuid.to_owned(),
                password: password.to_owned(),
            },
            transport: None,
            tls: Some(TlsConfig {
                enabled: true,
                server_name: Some("127.0.0.1".to_owned()),
                skip_cert_verify: Some(true),
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
