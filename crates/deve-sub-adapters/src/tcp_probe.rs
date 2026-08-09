//! TCP connect latency probe: measures the round-trip time for a TCP
//! connection to a node's endpoint.
//!
//! This is the simplest latency probe (NODE-012). It connects to
//! `node.endpoint.host:port`, measures the elapsed time, and classifies
//! errors into [`ErrorClass`]. See
//! `docs/plan/milestones/M7-probes-and-detection.md` §"Latency probe model".

use std::time::Duration;

use async_trait::async_trait;
use deve_sub_domain::{ErrorClass, LatencyProbe, LatencyResult, Node};
use tokio::net::TcpStream;
use tokio::time::Instant;

/// TCP connect latency probe adapter.
pub struct TcpConnectProbe;

impl TcpConnectProbe {
    /// Create a new TCP connect probe adapter.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for TcpConnectProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LatencyProbe for TcpConnectProbe {
    async fn probe(&self, node: &Node, timeout: Duration) -> LatencyResult {
        let node_id = node.id;
        let host = match &node.endpoint.host {
            deve_sub_domain::Host::Domain(d) => d.to_string(),
            deve_sub_domain::Host::Ipv4(ip) => ip.to_string(),
            deve_sub_domain::Host::Ipv6(ip) => ip.to_string(),
        };
        let port = node.endpoint.port;
        let addr = format!("{host}:{port}");

        let start = Instant::now();
        match tokio::time::timeout(timeout, TcpStream::connect(&addr)).await {
            Ok(Ok(_stream)) => {
                let rtt = start.elapsed().as_millis().min(u32::MAX as u128) as u32;
                LatencyResult {
                    node_id,
                    rtt_ms: Some(rtt),
                    error_class: ErrorClass::Ok,
                }
            }
            Ok(Err(e)) => {
                let class = classify_tcp_error(&e);
                LatencyResult {
                    node_id,
                    rtt_ms: None,
                    error_class: class,
                }
            }
            Err(_) => LatencyResult {
                node_id,
                rtt_ms: None,
                error_class: ErrorClass::Timeout,
            },
        }
    }
}

/// Classify a TCP connection error into an [`ErrorClass`].
fn classify_tcp_error(e: &std::io::Error) -> ErrorClass {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::ConnectionRefused => ErrorClass::Refused,
        ErrorKind::TimedOut => ErrorClass::Timeout,
        ErrorKind::NotFound => ErrorClass::DnsFailed,
        _ => {
            let msg = e.to_string().to_lowercase();
            if msg.contains("dns") || msg.contains("resolve") || msg.contains("name") {
                ErrorClass::DnsFailed
            } else if msg.contains("refused") || msg.contains("reset") {
                ErrorClass::Refused
            } else {
                ErrorClass::Timeout
            }
        }
    }
}
