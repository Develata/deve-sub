//! NaiveProxy protocol client for the real-proxy probe.
//!
//! NaiveProxy tunnels TCP via HTTP/1.1 `CONNECT` over TLS. The client sends
//! `CONNECT host:port HTTP/1.1` with `Proxy-Authorization: Basic
//! base64(user:pass)`, reads a `HTTP/1.1 200` response line, then the TLS
//! stream becomes a raw TCP relay. See the NaiveProxy protocol specification.
//!
//! "Naive must not be downgraded to a plain HTTP node" (plan §protocol_config),
//! so this client always uses TLS regardless of the node's TLS config.

use std::time::Duration;

use base64::Engine as _;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
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
    let (username, password) = match &node.authentication {
        Authentication::UserPassword { username, password } => (username, password),
        _ => return Err(ErrorClass::Refused),
    };

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

    let credentials =
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    let connect_req = format!(
        "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\nProxy-Authorization: Basic {creds}\r\nProxy-Connection: keep-alive\r\n\r\n",
        host = target.host(),
        port = target.port(),
        creds = credentials,
    );

    tls.write_all(connect_req.as_bytes())
        .await
        .map_err(|_| ErrorClass::Refused)?;

    let mut buf = [0u8; 64];
    let n = tls.read(&mut buf).await.map_err(|_| ErrorClass::Refused)?;
    if n == 0 || !buf.starts_with(b"HTTP/1.1 200") {
        return Err(ErrorClass::Refused);
    }

    Ok(Box::new(tls))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::real_proxy::RealProxyProbe;
    use crate::real_proxy::test_util::{LocalHttpTarget, TestCert};
    use deve_sub_domain::{
        Authentication, Endpoint, Host, LatencyProbe, NaiveProxyConfig, Node, NodeSource,
        ProtocolConfig, ProtocolKind, RegionAssignment, RegionMethod, TlsConfig, UdpCapability,
    };
    use deve_sub_kernel::{NodeId, Timestamp};
    use std::collections::BTreeMap;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn naiveproxy_round_trip() {
        let http_target = LocalHttpTarget::start().await;
        let target_port = http_target.addr().port();

        let cert = TestCert::generate();
        let acceptor = cert.acceptor();
        let naive_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let naive_addr = naive_listener.local_addr().expect("addr");

        let username = "test-user".to_owned();
        let password = "test-pass".to_owned();
        let srv_username = username.clone();
        let srv_password = password.clone();

        let server = tokio::spawn(async move {
            let (tcp, _) = naive_listener.accept().await.expect("accept");
            let mut tls = acceptor.accept(tcp).await.expect("tls accept");

            let mut buf = [0u8; 256];
            let n = tls.read(&mut buf).await.expect("read connect");
            let request = String::from_utf8_lossy(&buf[..n]);
            assert!(request.starts_with("CONNECT "), "got: {request}");
            assert!(
                request.contains("Proxy-Authorization: Basic "),
                "got: {request}"
            );

            let expected_creds = base64::engine::general_purpose::STANDARD
                .encode(format!("{srv_username}:{srv_password}"));
            assert!(
                request.contains(&expected_creds),
                "missing creds in: {request}"
            );

            tls.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .expect("write 200");

            let mut target_stream =
                tokio::net::TcpStream::connect(format!("127.0.0.1:{target_port}"))
                    .await
                    .expect("connect target");
            let _ = tokio::io::copy_bidirectional(&mut tls, &mut target_stream).await;
        });

        let node = build_naive_node(naive_addr, &username, &password);
        let target = TestTarget::new("127.0.0.1", target_port, "/");
        let probe = RealProxyProbe::with_target(target);
        let result = probe.probe(&node, Duration::from_secs(5)).await;

        assert_eq!(result.error_class, ErrorClass::Ok);
        assert!(result.rtt_ms.is_some(), "should have RTT");

        http_target.abort();
        server.abort();
    }

    #[allow(clippy::expect_used, reason = "test code")]
    fn build_naive_node(addr: std::net::SocketAddr, username: &str, password: &str) -> Node {
        Node {
            id: NodeId::new(),
            display_name: "test-naive".to_owned(),
            protocol: ProtocolKind::NaiveProxy,
            config: ProtocolConfig::NaiveProxy(NaiveProxyConfig {
                quic: None,
                http2: None,
                http3: None,
            }),
            endpoint: Endpoint {
                host: Host::Ipv4("127.0.0.1".parse().expect("ipv4")),
                port: addr.port(),
            },
            authentication: Authentication::UserPassword {
                username: username.to_owned(),
                password: password.to_owned(),
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
