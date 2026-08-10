//! Shared QUIC client helpers for the real-proxy probe.
//!
//! Hysteria2 and TUIC both dial a QUIC endpoint with skip-verify TLS and
//! `h3` ALPN, then wrap a bidi stream that owns the endpoint + connection
//! so they outlive the stream halves. This module centralizes that logic.

use std::sync::Arc;
use std::time::Duration;

use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ClientConfig, Connection, Endpoint, TransportConfig};

use deve_sub_domain::{ErrorClass, Node};

use super::tls::skip_verify_client_config;

/// Establish a QUIC connection to `node`'s endpoint using skip-verify TLS
/// with the `h3` ALPN. Returns the endpoint (which owns the bound UDP
/// socket) and the established connection. The caller must keep both alive
/// for the lifetime of any streams opened on the connection.
pub async fn quic_connect(node: &Node) -> Result<(Endpoint, Connection), ErrorClass> {
    let host = node.endpoint.host.uri_host();
    let port = node.endpoint.port;
    let addr = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|_| ErrorClass::DnsFailed)?
        .next()
        .ok_or(ErrorClass::DnsFailed)?;

    let tls =
        skip_verify_client_config(vec![b"h3".to_vec()]).map_err(|_| ErrorClass::QuicFailed)?;
    let quic_cfg = QuicClientConfig::try_from(tls).map_err(|_| ErrorClass::QuicFailed)?;
    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(10)));
    let mut client_cfg = ClientConfig::new(Arc::new(quic_cfg));
    client_cfg.transport_config(Arc::new(transport));

    let mut ep = Endpoint::client("0.0.0.0:0".parse().map_err(|_| ErrorClass::QuicFailed)?)
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

/// A bidi QUIC stream that owns the endpoint and connection so they outlive
/// the stream halves. `tokio::io::join` merges the recv and send halves into
/// a single `AsyncRead + AsyncWrite` object. Dropping this struct tears down
/// the connection and all its streams.
pub struct QuicBidiStream {
    pub(crate) inner: tokio::io::Join<quinn::RecvStream, quinn::SendStream>,
    pub(crate) _endpoint: quinn::Endpoint,
    pub(crate) _conn: quinn::Connection,
}

impl tokio::io::AsyncRead for QuicBidiStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for QuicBidiStream {
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
