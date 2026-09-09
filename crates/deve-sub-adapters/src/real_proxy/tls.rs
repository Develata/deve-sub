//! TLS policy for authenticated proxy probes; explicit opt-out only.

use std::sync::Arc;

use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::TlsConnector;

/// Explicit node-policy opt-out. Handshake signatures still verify possession
/// of the presented key; this mode does not authenticate its identity.
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

/// Build the connector under the canonical node TLS security policy.
pub fn connector(
    node: &deve_sub_domain::Node,
    alpn: Vec<Vec<u8>>,
) -> Result<TlsConnector, rustls::Error> {
    Ok(TlsConnector::from(Arc::new(client_config(node, alpn)?)))
}

/// Fail closed for unsupported security settings before sending credentials.
/// This probe does not implement pin interpretation or Reality authentication.
pub fn client_config(
    node: &deve_sub_domain::Node,
    alpn: Vec<Vec<u8>>,
) -> Result<rustls::ClientConfig, rustls::Error> {
    if node
        .tls
        .as_ref()
        .is_some_and(|tls| !tls.certificate_pins.is_empty() || tls.reality.is_some())
    {
        return Err(rustls::Error::General(
            "probe TLS security settings unsupported".into(),
        ));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?;
    let mut config = if node
        .tls
        .as_ref()
        .is_some_and(|tls| tls.skip_cert_verify == Some(true))
    {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipVerification(provider)))
            .with_no_client_auth()
    } else {
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    config.alpn_protocols = alpn;
    Ok(config)
}
