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
    /// IPv4 address.
    Ipv4(Ipv4Addr),
    /// IPv6 address. URI output auto-adds brackets.
    Ipv6(Ipv6Addr),
    /// Domain name.
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

    /// Parse a host string stored by [`Self::uri_host`] back into a [`Host`].
    ///
    /// IPv6 is stored bracketed (`[2001:db8::1]`); IPv4 and domains are
    /// stored bare. This is the inverse of `uri_host` and is used by the
    /// storage adapter to reconstruct [`Host`] from the `nodes.host` column.
    /// Brackets are stripped for IPv6; a bare IPv6 string (no brackets) is
    /// also accepted as a convenience.
    ///
    /// WHY: the dedup index stores `uri_host()` output, so reads must invert
    /// that exact representation. Treating a non-IPv4, non-IPv6 string as a
    /// domain matches the M3 parser's deferred-validation policy.
    #[must_use]
    pub fn parse_uri_host(s: &str) -> Self {
        if let Some(inner) = s.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if let Ok(addr) = inner.parse::<Ipv6Addr>() {
                return Self::Ipv6(addr);
            }
            return Self::Domain(DomainName::new(s.to_owned()));
        }
        if let Ok(addr) = s.parse::<Ipv4Addr>() {
            return Self::Ipv4(addr);
        }
        if let Ok(addr) = s.parse::<Ipv6Addr>() {
            return Self::Ipv6(addr);
        }
        Self::Domain(DomainName::new(s.to_owned()))
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

    #[test]
    fn parse_uri_host_roundtrips_all_variants() {
        for host in [
            Host::Ipv4("127.0.0.1".parse().expect("valid IPv4")),
            Host::Ipv6("2001:db8::1".parse().expect("valid IPv6")),
            Host::Domain(DomainName::new("example.com".to_owned())),
        ] {
            let s = host.uri_host();
            let recovered = Host::parse_uri_host(&s);
            assert_eq!(host, recovered, "roundtrip for {host:?}");
        }
    }

    #[test]
    fn parse_uri_host_accepts_bare_ipv6() {
        let host = Host::parse_uri_host("2001:db8::1");
        assert_eq!(host, Host::Ipv6("2001:db8::1".parse().expect("valid IPv6")));
    }

    #[test]
    fn parse_uri_host_bracketed_non_ipv6_falls_back_to_domain() {
        let host = Host::parse_uri_host("[not-an-ip]");
        assert_eq!(
            host,
            Host::Domain(DomainName::new("[not-an-ip]".to_owned()))
        );
    }
}
