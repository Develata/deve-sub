//! Shared TLS connector for TCP-based proxy protocols (Trojan, VLESS,
//! NaiveProxy). Reuses the skip-cert-verify pattern from `quic_probe.rs`
//! because the probe measures reachability, not authenticity.

use std::sync::Arc;

use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::TlsConnector;

/// A certificate verifier that accepts any server certificate. Proxy nodes
/// frequently use self-signed certificates; the real-proxy probe measures
/// reachability through the proxy, not server authenticity. This must not
/// be used outside probe contexts.
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

/// Build a `TlsConnector` that skips certificate verification. The probe
/// connects to proxy nodes that may use self-signed certs.
///
/// # Errors
/// Returns `Err` if the rustls client config cannot be built (should not
/// happen with the ring provider).
pub fn skip_verify_connector(alpn: Vec<Vec<u8>>) -> Result<TlsConnector, rustls::Error> {
    let config = skip_verify_client_config(alpn)?;
    Ok(TlsConnector::from(Arc::new(config)))
}

/// Build a raw `rustls::ClientConfig` that skips certificate verification.
/// Used by QUIC-based clients (Hysteria2, TUIC) that need the config for
/// `quinn` rather than a `tokio-rustls` connector.
///
/// # Errors
/// Returns `Err` if the rustls client config cannot be built (should not
/// happen with the ring provider).
pub fn skip_verify_client_config(
    alpn: Vec<Vec<u8>>,
) -> Result<rustls::ClientConfig, rustls::Error> {
    let verifier = Arc::new(SkipVerification(Arc::new(
        rustls::crypto::ring::default_provider(),
    )));
    let mut builder = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    builder.alpn_protocols = alpn;
    Ok(builder)
}
