//! Ordinary forward proxies, distinct from NaiveProxy.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Where the destination hostname is resolved for SOCKS5 CONNECT.
///
/// This never controls resolution of the proxy server's own hostname.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SocksDnsResolution {
    /// The source leaves destination resolution to the client configuration.
    #[default]
    Unspecified,
    /// Resolve locally and send an IP address (curl `socks5://`).
    Local,
    /// Send the hostname to the proxy (curl `socks5h://`).
    Remote,
}

/// SOCKS version 5; authentication and optional TLS live on the node.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Socks5Config {
    /// Explicit DNS intent must not disappear during conversion.
    #[serde(default)]
    pub dns_resolution: SocksDnsResolution,
}

/// HTTP CONNECT configuration, including HTTPS via the node's TLS settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HttpProxyConfig {
    /// Nonstandard CONNECT request path, supported by sing-box.
    #[serde(default)]
    pub path: Option<String>,
    /// Additional request headers; repeated values retain their order.
    #[serde(default)]
    pub headers: BTreeMap<String, Vec<String>>,
}
