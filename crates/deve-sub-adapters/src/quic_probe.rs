//! QUIC handshake latency probe: measures the round-trip time for a QUIC
//! connection handshake to a node's endpoint (HY2/TUIC).
//!
//! This is the QUIC latency probe (NODE-013). It performs a real QUIC
//! handshake to `node.endpoint`, measures the elapsed time, and classifies
//! errors into [`ErrorClass`]. For non-responsive UDP endpoints, the
//! handshake times out and the record stores `rtt_ms = None` +
//! `error_class = Timeout` — no fake latency, no auto-kill (NODE-014, spec
//! §98). See `docs/plan/milestones/M7-probes-and-detection.md`
//! §"Latency probe model".

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use async_trait::async_trait;
use deve_sub_domain::{ErrorClass, LatencyProbe, LatencyResult, Node};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ClientConfig, Endpoint, TransportConfig};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio::time::Instant;

/// QUIC handshake latency probe adapter.
///
/// A single [`quinn::Endpoint`] is lazily created on first use and reused
/// across all probe calls. The endpoint binds to `0.0.0.0:0` (ephemeral
/// source port) and is configured to skip server certificate verification,
/// because the probe measures reachability/latency, not authentication.
pub struct QuicHandshakeProbe {
    endpoint: OnceLock<Endpoint>,
}

impl QuicHandshakeProbe {
    /// Create a new QUIC handshake probe adapter. The underlying QUIC
    /// endpoint is created lazily on the first `probe` call.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            endpoint: OnceLock::new(),
        }
    }
}

impl Default for QuicHandshakeProbe {
    fn default() -> Self {
        Self::new()
    }
}

/// A certificate verifier that accepts any server certificate. The QUIC
/// latency probe measures reachability, not authenticity — proxy nodes
/// frequently use self-signed certificates. This must not be used outside
/// probe contexts.
struct SkipVerification(Arc<rustls::crypto::CryptoProvider>);

impl std::fmt::Debug for SkipVerification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkipVerification").finish_non_exhaustive()
    }
}

impl rustls::client::danger::ServerCertVerifier for SkipVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        msg: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            msg,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        msg: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            msg,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

impl QuicHandshakeProbe {
    /// Get or create the lazily-initialized QUIC endpoint.
    fn endpoint(&self) -> Result<&Endpoint, ErrorClass> {
        if let Some(ep) = self.endpoint.get() {
            return Ok(ep);
        }
        let verifier = Arc::new(SkipVerification(Arc::new(
            rustls::crypto::ring::default_provider(),
        )));
        let client_crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        let quic_config =
            QuicClientConfig::try_from(client_crypto).map_err(|_| ErrorClass::QuicFailed)?;
        let mut transport = TransportConfig::default();
        transport.keep_alive_interval(None);
        let mut client_config = ClientConfig::new(Arc::new(quic_config));
        client_config.transport_config(Arc::new(transport));
        let mut ep = Endpoint::client("0.0.0.0:0".parse().map_err(|_| ErrorClass::QuicFailed)?)
            .map_err(|_| ErrorClass::QuicFailed)?;
        ep.set_default_client_config(client_config);
        // Another task may have won the OnceLock race; either way the lock
        // is now populated, so get() is guaranteed Some. ok_or is a
        // fallback that never triggers under the OnceLock invariant.
        let _ = self.endpoint.set(ep);
        self.endpoint.get().ok_or(ErrorClass::QuicFailed)
    }
}

#[async_trait]
impl LatencyProbe for QuicHandshakeProbe {
    async fn probe(&self, node: &Node, timeout: Duration) -> LatencyResult {
        let node_id = node.id;

        // Resolve the endpoint to a SocketAddr (DNS if domain, direct if IP).
        let host_str = match &node.endpoint.host {
            deve_sub_domain::Host::Domain(d) => d.to_string(),
            deve_sub_domain::Host::Ipv4(ip) => ip.to_string(),
            deve_sub_domain::Host::Ipv6(ip) => ip.to_string(),
        };
        let port = node.endpoint.port;
        let addr_target = format!("{host_str}:{port}");

        let start = Instant::now();

        // DNS resolution (for domains) with the probe timeout.
        let socket_addr: SocketAddr = match tokio::time::timeout(timeout, async {
            tokio::net::lookup_host(&addr_target)
                .await
                .and_then(|mut iter| {
                    iter.next().ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::NotFound, "no addresses resolved")
                    })
                })
        })
        .await
        {
            Ok(Ok(addr)) => addr,
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return LatencyResult {
                    node_id,
                    rtt_ms: None,
                    error_class: ErrorClass::DnsFailed,
                };
            }
            Ok(Err(_)) => {
                return LatencyResult {
                    node_id,
                    rtt_ms: None,
                    error_class: ErrorClass::DnsFailed,
                };
            }
            Err(_) => {
                return LatencyResult {
                    node_id,
                    rtt_ms: None,
                    error_class: ErrorClass::Timeout,
                };
            }
        };

        // Get the QUIC endpoint (lazily initialized).
        let endpoint = match self.endpoint() {
            Ok(ep) => ep,
            Err(class) => {
                return LatencyResult {
                    node_id,
                    rtt_ms: None,
                    error_class: class,
                };
            }
        };

        // Server name for SNI: prefer explicit TLS server_name, fall back to host.
        let server_name = node
            .tls
            .as_ref()
            .and_then(|t| t.server_name.as_deref())
            .unwrap_or(&host_str);

        // Initiate the QUIC connection (handshake).
        let connecting = match endpoint.connect(socket_addr, server_name) {
            Ok(c) => c,
            Err(quinn::ConnectError::InvalidServerName(_)) => {
                return LatencyResult {
                    node_id,
                    rtt_ms: None,
                    error_class: ErrorClass::DnsFailed,
                };
            }
            Err(_) => {
                return LatencyResult {
                    node_id,
                    rtt_ms: None,
                    error_class: ErrorClass::QuicFailed,
                };
            }
        };

        // Await the handshake with the remaining timeout budget.
        let elapsed = start.elapsed();
        let remaining = timeout.saturating_sub(elapsed);
        match tokio::time::timeout(remaining, connecting).await {
            Ok(Ok(_conn)) => {
                let rtt = start.elapsed().as_millis().min(u32::MAX as u128) as u32;
                LatencyResult {
                    node_id,
                    rtt_ms: Some(rtt),
                    error_class: ErrorClass::Ok,
                }
            }
            Ok(Err(e)) => LatencyResult {
                node_id,
                rtt_ms: None,
                error_class: classify_quic_error(&e),
            },
            Err(_) => LatencyResult {
                node_id,
                rtt_ms: None,
                error_class: ErrorClass::Timeout,
            },
        }
    }
}

/// Classify a QUIC connection error into an [`ErrorClass`].
fn classify_quic_error(e: &quinn::ConnectionError) -> ErrorClass {
    use quinn::ConnectionError;
    match e {
        ConnectionError::TimedOut => ErrorClass::Timeout,
        ConnectionError::Reset => ErrorClass::Refused,
        ConnectionError::ConnectionClosed(_) | ConnectionError::ApplicationClosed(_) => {
            ErrorClass::QuicFailed
        }
        ConnectionError::TransportError(_)
        | ConnectionError::VersionMismatch
        | ConnectionError::LocallyClosed
        | ConnectionError::CidsExhausted => ErrorClass::QuicFailed,
    }
}
