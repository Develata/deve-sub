//! Real-proxy latency probe: dials *through* a proxy node using its protocol
//! config to a test HTTP target, sends a minimal HTTP request, and measures
//! end-to-end RTT.
//!
//! This is the most accurate latency metric (spec §94). Each P0 protocol has
//! its own client module under this directory. The `RealProxyProbe` struct
//! dispatches by [`ProtocolConfig`] variant to the appropriate client, then
//! performs a shared HTTP round-trip over the tunneled stream.
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` §"Latency probe
//! model" and NODE-015.

#[allow(dead_code)] // wrap_quinn_bidi used by Phase 4 QUIC clients
mod stream;
mod target;
mod tls;

// Protocol clients — added incrementally per phase:
mod shadowsocks; // Phase 2
mod trojan; // Phase 2
mod vless; // Phase 2
mod vmess; // Phase 3
// mod hysteria2;    // Phase 4
// mod tuic;         // Phase 4
// mod vless_reality; // Phase 5
// mod naiveproxy;   // Phase 5

#[cfg(test)]
mod test_util;

use std::time::Duration;

use async_trait::async_trait;
use deve_sub_domain::{ErrorClass, LatencyProbe, LatencyResult, Node, ProtocolConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;

pub use stream::BoxedStream;
pub use target::TestTarget;

/// Real-proxy latency probe adapter.
///
/// Dials through a proxy node to a test HTTP target and measures RTT. The
/// test target defaults to `www.gstatic.com:80/generate_204`; use
/// [`RealProxyProbe::with_target`] to override (e.g. for tests).
pub struct RealProxyProbe {
    test_target: TestTarget,
}

impl RealProxyProbe {
    /// Create a probe with the default test target.
    #[must_use]
    pub fn new() -> Self {
        Self {
            test_target: TestTarget::default(),
        }
    }

    /// Create a probe with a custom test target (for tests).
    #[must_use]
    pub fn with_target(target: TestTarget) -> Self {
        Self {
            test_target: target,
        }
    }
}

impl Default for RealProxyProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LatencyProbe for RealProxyProbe {
    async fn probe(&self, node: &Node, timeout: Duration) -> LatencyResult {
        let node_id = node.id;
        let start = Instant::now();

        let stream = match self.dial(node, timeout).await {
            Ok(s) => s,
            Err(class) => {
                return LatencyResult {
                    node_id,
                    rtt_ms: None,
                    error_class: class,
                };
            }
        };

        match self.http_round_trip(stream, timeout).await {
            Ok(()) => {
                let rtt = start.elapsed().as_millis().min(u32::MAX as u128) as u32;
                LatencyResult {
                    node_id,
                    rtt_ms: Some(rtt),
                    error_class: ErrorClass::Ok,
                }
            }
            Err(class) => LatencyResult {
                node_id,
                rtt_ms: None,
                error_class: class,
            },
        }
    }
}

impl RealProxyProbe {
    /// Dial through the proxy node to the test target, returning a
    /// tunneled stream. Dispatches by protocol config variant.
    async fn dial(&self, node: &Node, _timeout: Duration) -> Result<BoxedStream, ErrorClass> {
        match &node.config {
            ProtocolConfig::Trojan(_) => trojan::dial(node, &self.test_target, _timeout).await,
            ProtocolConfig::Shadowsocks(_) => {
                shadowsocks::dial(node, &self.test_target, _timeout).await
            }
            ProtocolConfig::VlessReality(_) => vless::dial(node, &self.test_target, _timeout).await,
            ProtocolConfig::VMess(_) => vmess::dial(node, &self.test_target, _timeout).await,
            ProtocolConfig::Hysteria2(_) => {
                tracing::debug!(
                    protocol = "hysteria2",
                    "real-proxy client not yet implemented"
                );
                Err(ErrorClass::Refused)
            }
            ProtocolConfig::TuicV5(_) => {
                tracing::debug!(protocol = "tuic", "real-proxy client not yet implemented");
                Err(ErrorClass::Refused)
            }
            ProtocolConfig::NaiveProxy(_) => {
                tracing::debug!(
                    protocol = "naiveproxy",
                    "real-proxy client not yet implemented"
                );
                Err(ErrorClass::Refused)
            }
            ProtocolConfig::Unsupported(_) => {
                tracing::debug!(
                    protocol = "unsupported",
                    "real-proxy probe cannot handle unsupported protocol"
                );
                Err(ErrorClass::Refused)
            }
            _ => {
                tracing::debug!(
                    protocol = "unknown",
                    "real-proxy probe cannot handle unknown protocol"
                );
                Err(ErrorClass::Refused)
            }
        }
    }

    /// Send a minimal HTTP HEAD request through the tunneled stream and
    /// read the first response line. The RTT is measured by the caller
    /// from dial-start to this point.
    async fn http_round_trip(
        &self,
        mut stream: BoxedStream,
        timeout: Duration,
    ) -> Result<(), ErrorClass> {
        let request = self.test_target.http_request_bytes();

        match tokio::time::timeout(timeout, stream.write_all(&request)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return Err(ErrorClass::Refused),
            Err(_) => return Err(ErrorClass::Timeout),
        }

        let mut buf = [0u8; 64];
        match tokio::time::timeout(timeout, stream.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 && buf.starts_with(b"HTTP/") => Ok(()),
            Ok(Ok(_)) => Err(ErrorClass::Refused),
            Ok(Err(_)) => Err(ErrorClass::Refused),
            Err(_) => Err(ErrorClass::Timeout),
        }
    }
}
