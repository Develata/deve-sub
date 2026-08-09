//! Stream helpers for wrapping QUIC bidi streams into a unified
//! `AsyncRead + AsyncWrite` boxed stream.

use tokio::io::{AsyncRead, AsyncWrite};

/// A stream that supports both `AsyncRead` and `AsyncWrite`, used as the
/// return type of protocol-specific dial functions. Rust trait objects
/// cannot combine two non-auto traits directly, so we use a supertrait.
pub trait ProxyStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> ProxyStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

/// A boxed stream that implements both `AsyncRead` and `AsyncWrite`.
pub type BoxedStream = Box<dyn ProxyStream>;

/// Wrap a quinn bidirectional stream pair into a `BoxedStream`.
pub fn wrap_quinn_bidi(send: quinn::SendStream, recv: quinn::RecvStream) -> BoxedStream {
    Box::new(tokio::io::join(recv, send))
}
