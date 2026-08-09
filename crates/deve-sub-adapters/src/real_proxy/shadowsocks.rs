//! Shadowsocks protocol client for the real-proxy probe.
//!
//! Uses the official `shadowsocks` crate's `ProxyClientStream::connect` to
//! dial through the SS server. The crate handles key derivation, AEAD
//! framing, and SOCKS5 address encoding for all supported ciphers.

use std::str::FromStr;
use std::time::Duration;

use shadowsocks::config::{ServerAddr, ServerConfig, ServerType};
use shadowsocks::context::Context;
use shadowsocks::crypto::CipherKind;
use shadowsocks::relay::Address;
use shadowsocks::relay::tcprelay::proxy_stream::ProxyClientStream;

use deve_sub_domain::{Authentication, ErrorClass, Node, ProtocolConfig, ShadowsocksConfig};

use super::stream::BoxedStream;
use super::target::TestTarget;

/// Dial through a Shadowsocks proxy node to `target`, returning a tunneled
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
    let (password, method_str) = match (&node.authentication, &node.config) {
        (
            Authentication::Password { password },
            ProtocolConfig::Shadowsocks(ShadowsocksConfig { method, .. }),
        ) => (password.clone(), method.clone()),
        _ => return Err(ErrorClass::Refused),
    };

    let method = CipherKind::from_str(&method_str).map_err(|_| ErrorClass::Refused)?;
    let svr_addr = ServerAddr::from((node.endpoint.host.uri_host(), node.endpoint.port));
    let svr_cfg = ServerConfig::new(svr_addr, password, method).map_err(|_| ErrorClass::Refused)?;

    let context = Context::new_shared(ServerType::Local);
    let target_addr = Address::from((target.host().to_owned(), target.port()));

    let stream = ProxyClientStream::connect(context, &svr_cfg, target_addr)
        .await
        .map_err(|_| ErrorClass::Refused)?;

    Ok(Box::new(stream))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::real_proxy::RealProxyProbe;
    use crate::real_proxy::test_util::LocalHttpTarget;
    use deve_sub_domain::{
        Authentication, Endpoint, Host, LatencyProbe, Node, NodeSource, ProtocolConfig,
        ProtocolKind, RegionAssignment, RegionMethod, ShadowsocksConfig, UdpCapability,
    };
    use deve_sub_kernel::{NodeId, Timestamp};
    use shadowsocks::relay::tcprelay::proxy_listener::ProxyListener;
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[tokio::test]
    async fn shadowsocks_round_trip() {
        let http_target = LocalHttpTarget::start().await;

        let probe_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let ss_addr = probe_listener.local_addr().expect("addr");
        drop(probe_listener);

        let password = "test-password";
        let method = CipherKind::AES_256_GCM;
        let svr_addr = ServerAddr::from(ss_addr);
        let svr_cfg = ServerConfig::new(svr_addr, password, method).expect("server config");
        let context = Context::new_shared(ServerType::Server);
        let proxy_listener = ProxyListener::bind(context, &svr_cfg).await.expect("bind");

        let target_port = http_target.addr().port();
        let server = tokio::spawn(async move {
            let (mut client_stream, _) = proxy_listener.accept().await.expect("accept");
            let target_addr = client_stream.handshake().await.expect("handshake");

            let target_sock = match &target_addr {
                Address::SocketAddress(sa) => *sa,
                Address::DomainNameAddress(host, port) => {
                    std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), *port))
                        .expect("resolve")
                        .next()
                        .expect("at least one addr")
                }
            };
            let mut target_stream = tokio::net::TcpStream::connect(target_sock)
                .await
                .expect("connect");
            let _ = tokio::io::copy_bidirectional(&mut client_stream, &mut target_stream).await;
        });

        let node = build_ss_node(ss_addr, "aes-256-gcm", password);
        let target = TestTarget::new("127.0.0.1", target_port, "/");
        let probe = RealProxyProbe::with_target(target);
        let result = probe.probe(&node, Duration::from_secs(10)).await;

        assert_eq!(result.error_class, ErrorClass::Ok);
        assert!(result.rtt_ms.is_some(), "should have RTT");

        http_target.abort();
        server.abort();
    }

    fn build_ss_node(addr: std::net::SocketAddr, method: &str, password: &str) -> Node {
        Node {
            id: NodeId::new(),
            display_name: "test-ss".to_owned(),
            protocol: ProtocolKind::Shadowsocks,
            config: ProtocolConfig::Shadowsocks(ShadowsocksConfig {
                method: method.to_owned(),
                plugin: None,
                plugin_opts: None,
            }),
            endpoint: Endpoint {
                host: Host::Ipv4("127.0.0.1".parse().expect("ipv4")),
                port: addr.port(),
            },
            authentication: Authentication::Password {
                password: password.to_owned(),
            },
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
