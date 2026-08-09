//! Test target for the real-proxy probe: the HTTP endpoint the probe dials
//! to through the proxy node to measure end-to-end RTT.

/// The HTTP endpoint a [`super::RealProxyProbe`] connects to through the
/// proxy node.
///
/// In production the default target is a stable public HTTP endpoint. In
/// tests a local HTTP server is used so the round-trip can be verified
/// without external network access.
#[derive(Debug, Clone)]
pub struct TestTarget {
    host: String,
    port: u16,
    path: String,
}

impl TestTarget {
    /// Create a new test target.
    #[must_use]
    pub fn new(host: impl Into<String>, port: u16, path: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port,
            path: path.into(),
        }
    }

    /// Target hostname (used for SOCKS/CONNECT address and HTTP `Host` header).
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Target port.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// HTTP request path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Build the raw HTTP/1.1 request bytes to send through the proxy.
    #[must_use]
    pub fn http_request_bytes(&self) -> Vec<u8> {
        format!(
            "HEAD {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nUser-Agent: deve-sub-probe\r\n\r\n",
            path = self.path,
            host = self.host,
            port = self.port,
        )
        .into_bytes()
    }
}

impl Default for TestTarget {
    fn default() -> Self {
        Self::new("www.gstatic.com", 80, "/generate_204")
    }
}
