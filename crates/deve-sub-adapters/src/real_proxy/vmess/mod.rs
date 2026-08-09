//! VMess AEAD protocol client for the real-proxy probe.
//!
//! VMess uses AEAD-encrypted headers and chunk-based body encryption. The
//! dial function seals the request header, opens a TLS connection, sends the
//! header, then spawns a background relay that encrypts/decrypts body records
//! through a `tokio::io::duplex` so the caller sees a plain stream.

pub mod body;
pub mod header;
pub mod kdf;

use std::time::Duration;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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
    let uuid = uuid::Uuid::parse_str(uuid_str).map_err(|_| ErrorClass::Refused)?;
    let cmd_key = header::cmd_key(uuid.as_bytes());

    let mut rng = StdRng::from_entropy();
    let sealed = header::seal_request_header(&cmd_key, target.port(), target.host(), &mut rng);

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

    tls.write_all(&sealed.bytes)
        .await
        .map_err(|_| ErrorClass::Refused)?;

    let body_cipher = body::BodyCipher::new(&sealed.req_key, &sealed.req_iv);
    let resp_v = sealed.resp_v;
    let req_key = sealed.req_key;
    let req_iv = sealed.req_iv;

    let (client, proxy) = tokio::io::duplex(32768);
    let (mut tls_rd, mut tls_wr) = tokio::io::split(tls);
    let (mut write_half, mut read_half) = body_cipher.split();
    let (mut proxy_rd, mut proxy_wr) = tokio::io::split(proxy);

    let read_task = tokio::spawn(async move {
        let (resp_body_key, resp_body_iv) = header::response_body_keys(&req_key, &req_iv);
        if header::read_aead_response_header(&mut tls_rd, &resp_body_key, &resp_body_iv, resp_v)
            .await
            .is_err()
        {
            return;
        }
        while let Ok(plaintext) = read_half.read_record(&mut tls_rd).await {
            if proxy_wr.write_all(&plaintext).await.is_err() {
                break;
            }
        }
        let _ = proxy_wr.shutdown().await;
    });

    let write_task = tokio::spawn(async move {
        let mut write_buf = vec![0u8; body::BodyCipher::max_plaintext()];
        loop {
            match proxy_rd.read(&mut write_buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if write_half
                        .write_record(&mut tls_wr, &write_buf[..n])
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        let _ = tls_wr.shutdown().await;
    });

    let relay = tokio::spawn(async move {
        let _ = read_task.await;
        write_task.abort();
    });

    Ok(Box::new(WrappedDuplex {
        inner: client,
        _relay: relay,
    }))
}

struct WrappedDuplex {
    inner: tokio::io::DuplexStream,
    _relay: tokio::task::JoinHandle<()>,
}

impl tokio::io::AsyncRead for WrappedDuplex {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for WrappedDuplex {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::real_proxy::RealProxyProbe;
    use crate::real_proxy::test_util::{LocalHttpTarget, TestCert};
    use aes_gcm::aead::{Aead, Payload};
    use aes_gcm::{Aes128Gcm, KeyInit as _, Nonce};
    use deve_sub_domain::{
        Authentication, Endpoint, Host, LatencyProbe, Node, NodeSource, ProtocolConfig,
        ProtocolKind, RegionAssignment, RegionMethod, TlsConfig, UdpCapability, VMessConfig,
    };
    use deve_sub_kernel::{NodeId, Timestamp};
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[tokio::test]
    async fn vmess_round_trip() {
        let http_target = LocalHttpTarget::start().await;
        let cert = TestCert::generate();
        let acceptor = cert.acceptor();
        let uuid_bytes: [u8; 16] = [
            0xb8, 0x31, 0x38, 0x1d, 0x63, 0x24, 0x4d, 0x53, 0xad, 0x4f, 0x8c, 0xda, 0x48, 0xb3,
            0x08, 0x11,
        ];
        let uuid = uuid::Uuid::from_bytes(uuid_bytes);
        let cmd_key = header::cmd_key(&uuid_bytes);

        let vmess_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let vmess_addr = vmess_listener.local_addr().expect("addr");
        let target_port = http_target.addr().port();

        let server = tokio::spawn(async move {
            let (tcp, _) = vmess_listener.accept().await.expect("accept");
            let mut tls = acceptor.accept(tcp).await.expect("tls accept");

            // Read auth_id (16) + encrypted_length (18) + conn_nonce (8) + encrypted_header.
            let mut auth_id = [0u8; 16];
            tls.read_exact(&mut auth_id).await.expect("auth_id");
            let mut enc_length = [0u8; 18];
            tls.read_exact(&mut enc_length).await.expect("enc_length");
            let mut conn_nonce = [0u8; 8];
            tls.read_exact(&mut conn_nonce).await.expect("conn_nonce");

            let length_key = kdf::kdf16(
                &cmd_key,
                &[b"VMess Header AEAD Key_Length", &auth_id, &conn_nonce],
            );
            let length_iv = kdf::kdf12(
                &cmd_key,
                &[b"VMess Header AEAD Nonce_Length", &auth_id, &conn_nonce],
            );
            let len_cipher = Aes128Gcm::new_from_slice(&length_key).expect("key");
            let len_pt = len_cipher
                .decrypt(
                    Nonce::from_slice(&length_iv),
                    Payload {
                        msg: &enc_length,
                        aad: &auth_id,
                    },
                )
                .expect("length decrypt");
            let hdr_len = u16::from_be_bytes([len_pt[0], len_pt[1]]) as usize;

            let mut enc_header = vec![0u8; hdr_len + 16];
            tls.read_exact(&mut enc_header).await.expect("enc_header");
            let header_key =
                kdf::kdf16(&cmd_key, &[b"VMess Header AEAD Key", &auth_id, &conn_nonce]);
            let header_iv = kdf::kdf12(
                &cmd_key,
                &[b"VMess Header AEAD Nonce", &auth_id, &conn_nonce],
            );
            let hdr_cipher = Aes128Gcm::new_from_slice(&header_key).expect("key");
            let hdr = hdr_cipher
                .decrypt(
                    Nonce::from_slice(&header_iv),
                    Payload {
                        msg: &enc_header,
                        aad: &auth_id,
                    },
                )
                .expect("header decrypt");

            // Parse header: version(1) + iv(16) + key(16) + resp_v(1) + opt(1) + p_sec(1) + reserved(1) + cmd(1) + port(2) + addr...
            let req_iv: [u8; 16] = hdr[1..17].try_into().expect("iv");
            let req_key: [u8; 16] = hdr[17..33].try_into().expect("key");
            let resp_v = hdr[33];
            let port = u16::from_be_bytes([hdr[38], hdr[39]]);
            let atyp = hdr[40];
            let target_addr = match atyp {
                0x01 => {
                    let mut octets = [0u8; 4];
                    octets.copy_from_slice(&hdr[41..45]);
                    std::net::SocketAddr::V4(std::net::SocketAddrV4::new(
                        std::net::Ipv4Addr::from(octets),
                        port,
                    ))
                }
                _ => panic!("unexpected atyp: {atyp}"),
            };

            // Read request body record (encrypted).
            let body_cipher = body::BodyCipher::new_server(&req_key, &req_iv);
            let (mut write_half, mut read_half) = body_cipher.split();
            let req_data = read_half.read_record(&mut tls).await.expect("body record");

            // Send response header.
            let (resp_key, resp_iv) = header::response_body_keys(&req_key, &req_iv);
            let len_key = kdf::kdf16(&resp_key, &[b"AEAD Resp Header Len Key"]);
            let len_iv = kdf::kdf12(&resp_iv, &[b"AEAD Resp Header Len IV"]);
            let len_ct = Aes128Gcm::new_from_slice(&len_key)
                .expect("key")
                .encrypt(Nonce::from_slice(&len_iv), (4u16).to_be_bytes().as_ref())
                .expect("len encrypt");
            tls.write_all(&len_ct).await.expect("write len");
            let hdr_key = kdf::kdf16(&resp_key, &[b"AEAD Resp Header Key"]);
            let hdr_iv = kdf::kdf12(&resp_iv, &[b"AEAD Resp Header IV"]);
            let resp_hdr = [resp_v, 0, 0, 0];
            let hdr_ct = Aes128Gcm::new_from_slice(&hdr_key)
                .expect("key")
                .encrypt(Nonce::from_slice(&hdr_iv), resp_hdr.as_ref())
                .expect("hdr encrypt");
            tls.write_all(&hdr_ct).await.expect("write hdr");

            // Connect to target and relay.
            let mut target_stream = tokio::net::TcpStream::connect(target_addr)
                .await
                .expect("connect");
            target_stream.write_all(&req_data).await.expect("send req");

            // Read HTTP response from target, encrypt as body record.
            let mut resp_buf = vec![0u8; 4096];
            let n = target_stream.read(&mut resp_buf).await.expect("read resp");
            write_half
                .write_record(&mut tls, &resp_buf[..n])
                .await
                .expect("body write");
        });

        let node = build_vmess_node(vmess_addr, &uuid.to_string());
        let target = TestTarget::new("127.0.0.1", target_port, "/");
        let probe = RealProxyProbe::with_target(target);
        let result = probe.probe(&node, Duration::from_secs(10)).await;

        assert_eq!(result.error_class, ErrorClass::Ok);
        assert!(result.rtt_ms.is_some(), "should have RTT");

        http_target.abort();
        server.abort();
    }

    fn build_vmess_node(addr: std::net::SocketAddr, uuid: &str) -> Node {
        Node {
            id: NodeId::new(),
            display_name: "test-vmess".to_owned(),
            protocol: ProtocolKind::VMess,
            config: ProtocolConfig::VMess(VMessConfig {
                alter_id: Some(0),
                security: Some("aes-128-gcm".to_owned()),
                packet_encoding: None,
            }),
            endpoint: Endpoint {
                host: Host::Ipv4("127.0.0.1".parse().expect("ipv4")),
                port: addr.port(),
            },
            authentication: Authentication::Uuid {
                uuid: uuid.to_owned(),
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
