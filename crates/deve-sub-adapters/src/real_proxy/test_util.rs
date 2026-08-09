//! Shared test infrastructure for real-proxy protocol round-trip tests.
#![cfg(test)]

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A minimal HTTP/1.1 server that responds `200 OK` to any request.
/// Used as the test target that the probe dials to through the proxy.
pub struct LocalHttpTarget {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl LocalHttpTarget {
    /// Start a local HTTP target on `127.0.0.1:0`.
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = sock.read(&mut buf).await;
                    let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
                    let _ = sock.write_all(resp).await;
                });
            }
        });
        Self { addr, handle }
    }

    #[must_use]
    pub const fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn abort(&self) {
        self.handle.abort();
    }
}

/// A self-signed TLS certificate + key for test servers.
pub struct TestCert {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

impl TestCert {
    /// Generate a self-signed certificate for `127.0.0.1`.
    pub fn generate() -> Self {
        let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into(), "localhost".into()])
            .expect("rcgen");
        Self {
            cert_der: cert.cert.der().to_vec(),
            key_der: cert.key_pair.serialize_der(),
        }
    }

    /// Build a `tokio_rustls::TlsAcceptor` for a test TLS server.
    pub fn acceptor(&self) -> tokio_rustls::TlsAcceptor {
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(self.key_der.clone().into());
        let cert = rustls::pki_types::CertificateDer::from(self.cert_der.clone());
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("server config");
        tokio_rustls::TlsAcceptor::from(Arc::new(config))
    }
}
