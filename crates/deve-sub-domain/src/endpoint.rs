//! Node endpoint addressing: host and port.
//!
//! IPv6 output must auto-add brackets in URI form. The database must not
//! store IPv6 as arbitrary strings for later concatenation. See ADR-0003.

use std::net::{Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};

/// The host portion of a node endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Host {
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
    Domain(DomainName),
}

impl Host {
    /// Return the bracketed form for URI embedding.
    ///
    /// IPv6 addresses are wrapped in `[...]`; IPv4 and domains are returned
    /// as-is. This is required for correct URI output like
    /// `vless://uuid@[2001:db8::1]:443`.
    #[must_use]
    pub fn uri_host(&self) -> String {
        match self {
            Self::Ipv4(addr) => addr.to_string(),
            Self::Ipv6(addr) => format!("[{addr}]"),
            Self::Domain(d) => d.to_string(),
        }
    }
}

/// A DNS domain name. Validation is deferred to the M3 parsing layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DomainName(String);

impl DomainName {
    /// Create from a string without validation. Validation arrives with the
    /// parsing layer in M3.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// Return the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DomainName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A network endpoint: host plus port.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Endpoint {
    /// Host portion of the endpoint (IPv4, IPv6, or domain).
    pub host: Host,
    /// TCP/UDP port number (0–65535).
    pub port: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv6_uri_host_has_brackets() {
        let host = Host::Ipv6("2001:db8::1".parse().expect("valid IPv6"));
        assert_eq!(host.uri_host(), "[2001:db8::1]");
    }

    #[test]
    fn ipv4_uri_host_no_brackets() {
        let host = Host::Ipv4("127.0.0.1".parse().expect("valid IPv4"));
        assert_eq!(host.uri_host(), "127.0.0.1");
    }

    #[test]
    fn domain_uri_host() {
        let host = Host::Domain(DomainName::new("example.com".to_owned()));
        assert_eq!(host.uri_host(), "example.com");
    }

    #[test]
    fn host_serde_roundtrip_all_variants() {
        for host in [
            Host::Ipv4("127.0.0.1".parse().expect("valid IPv4")),
            Host::Ipv6("2001:db8::1".parse().expect("valid IPv6")),
            Host::Domain(DomainName::new("example.com".to_owned())),
        ] {
            let json = serde_json::to_string(&host).expect("serialize");
            let recovered: Host = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(host, recovered);
        }
    }
}
