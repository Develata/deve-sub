//! Hysteria2 v2 protocol client for the real-proxy probe.
//!
//! Two-stage design (per PROTOCOL.md):
//! 1. HTTP/3 `POST /auth` with `Hysteria-Auth: <password>` authenticates the
//!    QUIC connection. Server responds with status `233`.
//! 2. Each TCP connection opens a new QUIC bidi stream carrying a `0x401`
//!    TCPRequest frame: `[varint 0x401][varint addr_len][addr][varint pad_len][pad]`.
//!
//! ALPN is `"h3"`. Auth uses the h3 crate; TCP proxying uses raw quinn
//! bidi streams. The h3 driver task is spawned for auth only and dropped
//! afterwards — it does not close the QUIC transport.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ClientConfig, Connection, Endpoint, TransportConfig};
use rand::{RngCore, SeedableRng};

use deve_sub_domain::{Authentication, ErrorClass, Node};

use super::stream::BoxedStream;
use super::target::TestTarget;
use super::tls::skip_verify_client_config;

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
        Authentication::Password { password } => password.clone(),
        _ => return Err(ErrorClass::Refused),
    };

    let (endpoint, conn) = quic_connect(node).await?;
    authenticate(&conn, &password).await?;
    let (send, recv) = open_tcp_stream(&conn, target).await?;
    Ok(Box::new(Hysteria2Stream {
        inner: tokio::io::join(recv, send),
        _endpoint: endpoint,
        _conn: conn,
    }))
}

async fn quic_connect(node: &Node) -> Result<(Endpoint, Connection), ErrorClass> {
    let host = node.endpoint.host.uri_host();
    let port = node.endpoint.port;
    let addr = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|_| ErrorClass::DnsFailed)?
        .next()
        .ok_or(ErrorClass::DnsFailed)?;

    let tls = skip_verify_client_config(vec![b"h3".to_vec()]).map_err(|_| ErrorClass::QuicFailed)?;
    let quic_cfg = QuicClientConfig::try_from(tls).map_err(|_| ErrorClass::QuicFailed)?;
    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(10)));
    let mut client_cfg = ClientConfig::new(Arc::new(quic_cfg));
    client_cfg.transport_config(Arc::new(transport));

    let mut ep =
        Endpoint::client("0.0.0.0:0".parse().map_err(|_| ErrorClass::QuicFailed)?)
            .map_err(|_| ErrorClass::QuicFailed)?;
    ep.set_default_client_config(client_cfg);

    let sni = node
        .tls
        .as_ref()
        .and_then(|t| t.server_name.as_deref())
        .unwrap_or(&host);
    let conn = ep
        .connect(addr, sni)
        .map_err(|_| ErrorClass::DnsFailed)?
        .await
        .map_err(|_| ErrorClass::QuicFailed)?;
    Ok((ep, conn))
}

async fn authenticate(conn: &Connection, password: &str) -> Result<(), ErrorClass> {
    let (_driver, mut send_request) =
        h3::client::new(h3_quinn::Connection::new(conn.clone()))
            .await
            .map_err(|_| ErrorClass::QuicFailed)?;

    let mut rng = rand::rngs::StdRng::from_entropy();
    let mut pad = vec![0u8; 16];
    rng.fill_bytes(&mut pad);
    let padding = base64::engine::general_purpose::STANDARD_NO_PAD.encode(&pad);

    let req = http::Request::builder()
        .method("POST")
        .uri("https://hysteria/auth")
        .header("Hysteria-Auth", password)
        .header("Hysteria-CC-RX", "0")
        .header("Hysteria-Padding", &padding)
        .body(())
        .map_err(|_| ErrorClass::QuicFailed)?;

    let mut stream = send_request
        .send_request(req)
        .await
        .map_err(|_| ErrorClass::Refused)?;
    stream.finish().await.map_err(|_| ErrorClass::Refused)?;

    let resp = stream.recv_response().await.map_err(|_| ErrorClass::Refused)?;
    if resp.status().as_u16() != 233 {
        return Err(ErrorClass::Refused);
    }
    Ok(())
}

async fn open_tcp_stream(
    conn: &Connection,
    target: &TestTarget,
) -> Result<(quinn::SendStream, quinn::RecvStream), ErrorClass> {
    let (mut send, recv) = conn.open_bi().await.map_err(|_| ErrorClass::Refused)?;

    let addr = format!("{}:{}", target.host(), target.port());
    let mut frame = Vec::with_capacity(4 + addr.len() + 4);
    write_varint(&mut frame, 0x401);
    write_varint(&mut frame, addr.len() as u64);
    frame.extend_from_slice(addr.as_bytes());

    let mut rng = rand::rngs::StdRng::from_entropy();
    let pad_len = (rng.next_u32() % 64) as u8;
    write_varint(&mut frame, pad_len as u64);
    let mut pad = vec![0u8; pad_len as usize];
    rng.fill_bytes(&mut pad);
    frame.extend_from_slice(&pad);

    send.write_all(&frame).await.map_err(|_| ErrorClass::Refused)?;
    Ok((send, recv))
}

/// Owns the QUIC endpoint + connection so they outlive the stream halves.
/// Dropping the endpoint tears down all connections, so it must live at
/// least as long as the bidi streams. `tokio::io::join` merges the recv
/// and send halves into a single `AsyncRead + AsyncWrite` object.
struct Hysteria2Stream {
    inner: tokio::io::Join<quinn::RecvStream, quinn::SendStream>,
    _endpoint: quinn::Endpoint,
    _conn: quinn::Connection,
}

impl tokio::io::AsyncRead for Hysteria2Stream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for Hysteria2Stream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

/// Encode a QUIC variable-length integer (RFC 9000 §16).
fn write_varint(buf: &mut Vec<u8>, val: u64) {
    if val < 64 {
        buf.push(val as u8);
    } else if val < 16384 {
        buf.push(0x40 | ((val >> 8) as u8));
        buf.push(val as u8);
    } else if val < 1_073_741_824 {
        buf.push(0x80 | ((val >> 24) as u8));
        buf.push((val >> 16) as u8);
        buf.push((val >> 8) as u8);
        buf.push(val as u8);
    } else {
        buf.push(0xC0 | ((val >> 56) as u8));
        buf.extend_from_slice(&val.to_be_bytes()[1..]);
    }
}

/// Decode a QUIC variable-length integer (RFC 9000 §16).
#[cfg(test)]
fn read_varint(buf: &mut &[u8]) -> Option<u64> {
    let first = *buf.first()?;
    let len = 1usize << (first >> 6);
    if buf.len() < len {
        return None;
    }
    let bytes = &buf[..len];
    *buf = &buf[len..];
    Some(match len {
        1 => u64::from(first & 0x3F),
        2 => u64::from(u16::from_be_bytes([bytes[0] & 0x3F, bytes[1]])),
        4 => u64::from(u32::from_be_bytes([
            bytes[0] & 0x3F,
            bytes[1],
            bytes[2],
            bytes[3],
        ])),
        8 => u64::from_be_bytes([
            bytes[0] & 0x3F,
            bytes[1],
            bytes[2],
            bytes[3],
            bytes[4],
            bytes[5],
            bytes[6],
            bytes[7],
        ]),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::real_proxy::test_util::{LocalHttpTarget, TestCert};
    use crate::real_proxy::RealProxyProbe;
    use deve_sub_domain::{
        Authentication, Endpoint, Hysteria2Config, Host, LatencyProbe, Node, NodeSource,
        ProtocolConfig, ProtocolKind, RegionAssignment, RegionMethod, TlsConfig, UdpCapability,
    };
    use deve_sub_kernel::{NodeId, Timestamp};
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn hysteria2_round_trip() {
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
        let qsc =
            quinn::crypto::rustls::QuicServerConfig::try_from(tls_srv).expect("quic server");
        let server_cfg = quinn::ServerConfig::with_crypto(Arc::new(qsc));
        let srv_ep =
            quinn::Endpoint::server(server_cfg, "127.0.0.1:0".parse().expect("parse"))
                .expect("server endpoint");
        let hy2_addr = srv_ep.local_addr().expect("addr");

        let password = "test-password".to_owned();
        let server_password = password.clone();

        let server = tokio::spawn(async move {
            let conn = srv_ep.accept().await.expect("accept").await.expect("conn");

            let mut h3_srv =
                h3::server::builder().build::<_, bytes::Bytes>(h3_quinn::Connection::new(conn.clone()))
                    .await
                    .expect("h3 server");

            let resolver = h3_srv.accept().await.expect("accept").expect("req");
            let (req, mut rs) = resolver.resolve_request().await.expect("resolve");
            assert_eq!(req.method(), "POST");
            assert_eq!(req.uri().path(), "/auth");
            let auth = req.headers().get("Hysteria-Auth").expect("auth header");
            assert_eq!(auth.to_str().expect("str"), server_password);

            let resp = http::Response::builder().status(233).body(()).expect("resp");
            rs.send_response(resp).await.expect("send resp");
            rs.finish().await.expect("finish");

            let (send, mut recv) = conn.accept_bi().await.expect("accept_bi");

            let id = read_varint_stream(&mut recv).await.expect("read id");
            assert_eq!(id, 0x401);

            let addr_len =
                read_varint_stream(&mut recv).await.expect("read addr_len") as usize;
            let mut addr_buf = vec![0u8; addr_len];
            recv.read_exact(&mut addr_buf).await.expect("read addr");
            let target_addr = String::from_utf8(addr_buf).expect("addr");

            let pad_len =
                read_varint_stream(&mut recv).await.expect("read pad_len") as usize;
            let mut pad = vec![0u8; pad_len];
            recv.read_exact(&mut pad).await.expect("read pad");

            let mut target =
                tokio::net::TcpStream::connect(target_addr).await.expect("connect");
            let mut combined = tokio::io::join(recv, send);
            let _ = tokio::io::copy_bidirectional(&mut combined, &mut target).await;
        });

        let node = build_hy2_node(hy2_addr, &password);
        let target = TestTarget::new("127.0.0.1", target_port, "/");
        let probe = RealProxyProbe::with_target(target);
        let result = probe.probe(&node, Duration::from_secs(10)).await;

        assert_eq!(result.error_class, ErrorClass::Ok);
        assert!(result.rtt_ms.is_some(), "should have RTT");

        http_target.abort();
        server.abort();
    }

    async fn read_varint_stream(recv: &mut quinn::RecvStream) -> Option<u64> {
        let mut first = [0u8; 1];
        recv.read_exact(&mut first).await.ok()?;
        let len = 1usize << (first[0] >> 6);
        let mut buf = vec![0u8; len];
        buf[0] = first[0];
        recv.read_exact(&mut buf[1..]).await.ok()?;
        let mut slice = buf.as_slice();
        read_varint(&mut slice)
    }

    #[allow(clippy::expect_used, reason = "test code")]
    fn build_hy2_node(addr: std::net::SocketAddr, password: &str) -> Node {
        Node {
            id: NodeId::new(),
            display_name: "test-hy2".to_owned(),
            protocol: ProtocolKind::Hysteria2,
            config: ProtocolConfig::Hysteria2(Hysteria2Config {
                ports: None,
                hop_interval: None,
                fast_open: None,
                lazy: None,
            }),
            endpoint: Endpoint {
                host: Host::Ipv4("127.0.0.1".parse().expect("ipv4")),
                port: addr.port(),
            },
            authentication: Authentication::Password {
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

    #[test]
    fn varint_round_trip() {
        for val in [0u64, 1, 63, 64, 16383, 16384, 0x401, 1_073_741_823] {
            let mut buf = Vec::new();
            write_varint(&mut buf, val);
            let mut slice = buf.as_slice();
            let recovered = read_varint(&mut slice).expect("read");
            assert_eq!(val, recovered, "varint {val}");
        }
    }

    #[test]
    fn base64_url_safe_no_pad() {
        let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode([0xF8, 0x3E, 0x51]);
        assert!(!encoded.contains('='));
    }
}
