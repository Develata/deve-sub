//! Stream type for the real-proxy probe: a boxed `AsyncRead + AsyncWrite`.

use tokio::io::{AsyncRead, AsyncWrite};

pub trait ProxyStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> ProxyStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub type BoxedStream = Box<dyn ProxyStream>;
