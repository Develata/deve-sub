//! SSRF (Server-Side Request Forgery) protection.
//!
//! The [`SsrfGuard`] resolves a URL's hostname to IP addresses and rejects
//! any that fall into blocked ranges (loopback, private, link-local,
//! multicast, unspecified, CGNAT, IPv6 ULA). The guard returns the safe IPs
//! so the HTTP fetcher can pin one, preventing DNS rebinding attacks.
//!
//! See `docs/plan/milestones/M4-sources-and-node-pool.md` §"SSRF guard"
//! and acceptance cases SEC-001 through SEC-005.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use thiserror::Error;
use url::Url;

/// Errors produced by SSRF validation.
#[derive(Debug, Error)]
pub enum SsrfError {
    /// The URL was invalid or missing a hostname.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// A resolved IP falls into a blocked range.
    #[error("SSRF blocked: {0}")]
    Blocked(String),

    /// The hostname could not be resolved (no DNS records).
    #[error("DNS resolution failed for {0}")]
    DnsResolutionFailed(String),

    /// A DNS lookup I/O error occurred.
    #[error("DNS lookup error: {0}")]
    DnsLookup(String),
}

/// SSRF guard: validates that a URL's resolved IPs are not in blocked ranges.
///
/// The guard is stateless and safe to share across threads. Call [`check`]
/// before any outbound HTTP request to a user-provided subscription URL.
/// The returned IPs are safe to connect to; the HTTP fetcher should pin one
/// to prevent DNS rebinding (SEC-003).
pub struct SsrfGuard;

impl SsrfGuard {
    /// Validate a URL and return the resolved safe IPs.
    ///
    /// Parses the URL, resolves the hostname (or checks an IP literal
    /// directly), and checks each IP against blocked ranges. Returns the
    /// list of safe IPs for the fetcher to pin.
    ///
    /// # Errors
    /// - [`SsrfError::InvalidUrl`] — the URL is malformed or has no host.
    /// - [`SsrfError::Blocked`] — a resolved IP is in a blocked range.
    /// - [`SsrfError::DnsResolutionFailed`] — the hostname has no DNS records.
    /// - [`SsrfError::DnsLookup`] — a DNS I/O error occurred.
    pub async fn check(url: &str) -> Result<Vec<IpAddr>, SsrfError> {
        let parsed = Url::parse(url).map_err(|e| SsrfError::InvalidUrl(e.to_string()))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| SsrfError::InvalidUrl("URL has no hostname".to_owned()))?;

        // If the host is an IP literal, check it directly without DNS.
        if let Ok(ip) = host.parse::<IpAddr>() {
            check_ip(&ip).map_err(SsrfError::Blocked)?;
            return Ok(vec![ip]);
        }

        // WHY: use port 0 for the lookup since we only need the IP addresses,
        // not a specific port. The actual port comes from the URL.
        let lookup_target = format!("{host}:0");
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host(&lookup_target)
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    SsrfError::DnsResolutionFailed(host.to_owned())
                } else {
                    SsrfError::DnsLookup(e.to_string())
                }
            })?
            .collect();

        if addrs.is_empty() {
            return Err(SsrfError::DnsResolutionFailed(host.to_owned()));
        }

        let mut safe_ips = Vec::with_capacity(addrs.len());
        for addr in &addrs {
            check_ip(&addr.ip()).map_err(SsrfError::Blocked)?;
            safe_ips.push(addr.ip());
        }

        Ok(safe_ips)
    }

    /// Check a single IP against blocked ranges (synchronous, no DNS).
    ///
    /// Convenience method for testing and for callers that already have
    /// a resolved IP.
    ///
    /// # Errors
    /// Returns [`SsrfError::Blocked`] with a description of the matched range.
    pub fn check_ip(ip: &IpAddr) -> Result<(), SsrfError> {
        check_ip(ip).map_err(SsrfError::Blocked)
    }
}

/// Check if an IP address is in a blocked range.
fn check_ip(ip: &IpAddr) -> Result<(), String> {
    match ip {
        IpAddr::V4(v4) => check_ipv4(v4),
        IpAddr::V6(v6) => check_ipv6(v6),
    }
}

/// Check an IPv4 address against blocked ranges.
fn check_ipv4(ip: &Ipv4Addr) -> Result<(), String> {
    if ip.is_loopback() {
        return Err("loopback address (127.0.0.0/8)".to_owned());
    }
    if ip.is_private() {
        return Err("private network (RFC 1918)".to_owned());
    }
    if ip.is_link_local() {
        return Err("link-local address (169.254.0.0/16)".to_owned());
    }
    if ip.is_multicast() {
        return Err("multicast address (224.0.0.0/4)".to_owned());
    }
    if ip.is_unspecified() {
        return Err("unspecified address (0.0.0.0)".to_owned());
    }
    // WHY: CGNAT (100.64.0.0/10) is not covered by std::is_private.
    if is_cgnat(ip) {
        return Err("CGNAT range (100.64.0.0/10)".to_owned());
    }
    // WHY: 0.0.0.0/8 "this network" is only partially covered by
    // is_unspecified() (which only matches 0.0.0.0 itself).
    if ip.octets()[0] == 0 {
        return Err("\"this network\" range (0.0.0.0/8)".to_owned());
    }
    Ok(())
}

/// Check an IPv6 address against blocked ranges.
fn check_ipv6(ip: &Ipv6Addr) -> Result<(), String> {
    if ip.is_loopback() {
        return Err("loopback address (::1)".to_owned());
    }
    if ip.is_multicast() {
        return Err("multicast address (ff00::/8)".to_owned());
    }
    if ip.is_unspecified() {
        return Err("unspecified address (::)".to_owned());
    }
    if ip.is_unicast_link_local() {
        return Err("link-local address (fe80::/10)".to_owned());
    }
    // WHY: IPv6 Unique Local Addresses (fc00::/7) are not covered by any
    // std method. Manual check needed.
    if is_ipv6_ula(ip) {
        return Err("unique local address (fc00::/7)".to_owned());
    }
    // WHY: IPv4-mapped IPv6 addresses (::ffff:a.b.c.d) can bypass IPv4
    // checks if not decomposed. Re-check as IPv4.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return check_ipv4(&v4);
    }
    // WHY: IPv4-compatible IPv6 addresses (::a.b.c.d, RFC 4291, deprecated)
    // are not caught by to_ipv4_mapped (which only matches ::ffff:0:0/96).
    // Modern OSes generally don't route them, but a malicious DNS AAAA
    // record could return ::7f00:1 to reach loopback. to_ipv4() catches
    // both mapped and compatible forms; guard against it here.
    if let Some(v4) = ip.to_ipv4() {
        return check_ipv4(&v4);
    }
    Ok(())
}

/// Check if an IPv4 address is in the CGNAT range (100.64.0.0/10).
fn is_cgnat(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (octets[1] & 0xc0) == 0x40
}

/// Check if an IPv6 address is in the ULA range (fc00::/7).
fn is_ipv6_ula(ip: &Ipv6Addr) -> bool {
    let segments = ip.segments();
    (segments[0] & 0xfe00) == 0xfc00
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_loopback_blocked() {
        assert!(check_ipv4(&Ipv4Addr::new(127, 0, 0, 1)).is_err());
        assert!(check_ipv4(&Ipv4Addr::new(127, 255, 255, 254)).is_err());
    }

    #[test]
    fn ipv4_private_blocked() {
        assert!(check_ipv4(&Ipv4Addr::new(10, 0, 0, 1)).is_err());
        assert!(check_ipv4(&Ipv4Addr::new(172, 16, 0, 1)).is_err());
        assert!(check_ipv4(&Ipv4Addr::new(192, 168, 1, 1)).is_err());
    }

    #[test]
    fn ipv4_link_local_blocked() {
        assert!(check_ipv4(&Ipv4Addr::new(169, 254, 1, 1)).is_err());
    }

    #[test]
    fn ipv4_multicast_blocked() {
        assert!(check_ipv4(&Ipv4Addr::new(224, 0, 0, 1)).is_err());
    }

    #[test]
    fn ipv4_unspecified_blocked() {
        assert!(check_ipv4(&Ipv4Addr::UNSPECIFIED).is_err());
    }

    #[test]
    fn ipv4_cgnat_blocked() {
        assert!(check_ipv4(&Ipv4Addr::new(100, 64, 0, 1)).is_err());
        assert!(check_ipv4(&Ipv4Addr::new(100, 127, 255, 255)).is_err());
    }

    #[test]
    fn ipv4_this_network_blocked() {
        assert!(check_ipv4(&Ipv4Addr::new(0, 1, 2, 3)).is_err());
    }

    #[test]
    fn ipv4_public_allowed() {
        assert!(check_ipv4(&Ipv4Addr::new(8, 8, 8, 8)).is_ok());
        assert!(check_ipv4(&Ipv4Addr::new(1, 1, 1, 1)).is_ok());
        assert!(check_ipv4(&Ipv4Addr::new(93, 184, 216, 34)).is_ok());
    }

    #[test]
    fn ipv6_loopback_blocked() {
        assert!(check_ipv6(&Ipv6Addr::LOCALHOST).is_err());
    }

    #[test]
    fn ipv6_multicast_blocked() {
        assert!(check_ipv6(&"ff02::1".parse::<Ipv6Addr>().expect("valid IPv6")).is_err());
    }

    #[test]
    fn ipv6_unspecified_blocked() {
        assert!(check_ipv6(&Ipv6Addr::UNSPECIFIED).is_err());
    }

    #[test]
    fn ipv6_link_local_blocked() {
        assert!(check_ipv6(&"fe80::1".parse::<Ipv6Addr>().expect("valid IPv6")).is_err());
    }

    #[test]
    fn ipv6_ula_blocked() {
        assert!(check_ipv6(&"fc00::1".parse::<Ipv6Addr>().expect("valid IPv6")).is_err());
        assert!(check_ipv6(&"fd00::1".parse::<Ipv6Addr>().expect("valid IPv6")).is_err());
    }

    #[test]
    fn ipv6_ipv4_mapped_loopback_blocked() {
        // WHY: ::ffff:127.0.0.1 must be blocked as loopback, not pass through
        // as a "safe" IPv6 address.
        let mapped = "::ffff:127.0.0.1".parse::<Ipv6Addr>().expect("valid IPv6");
        assert!(check_ipv6(&mapped).is_err());
    }

    #[test]
    fn ipv6_ipv4_compatible_loopback_blocked() {
        // WHY: ::127.0.0.1 (IPv4-compatible, deprecated) must be blocked.
        // to_ipv4_mapped() does NOT catch this form; to_ipv4() does.
        let compatible = "::127.0.0.1".parse::<Ipv6Addr>().expect("valid IPv6");
        assert!(check_ipv6(&compatible).is_err());
    }

    #[test]
    fn ipv6_ipv4_compatible_private_blocked() {
        let compatible = "::10.0.0.1".parse::<Ipv6Addr>().expect("valid IPv6");
        assert!(check_ipv6(&compatible).is_err());
    }

    #[test]
    fn ipv6_public_allowed() {
        assert!(
            check_ipv6(
                &"2606:4700:4700::1111"
                    .parse::<Ipv6Addr>()
                    .expect("valid IPv6")
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn check_ip_literal_loopback() {
        let result = SsrfGuard::check("http://127.0.0.1:8080/path").await;
        assert!(matches!(result, Err(SsrfError::Blocked(_))));
    }

    #[tokio::test]
    async fn check_ip_literal_private() {
        let result = SsrfGuard::check("http://10.0.0.1:8080/path").await;
        assert!(matches!(result, Err(SsrfError::Blocked(_))));
    }

    #[tokio::test]
    async fn check_ip_literal_ipv6_loopback() {
        let result = SsrfGuard::check("http://[::1]:8080/path").await;
        assert!(matches!(result, Err(SsrfError::Blocked(_))));
    }

    #[tokio::test]
    async fn check_ip_literal_ipv6_ula() {
        let result = SsrfGuard::check("http://[fc00::1]:8080/path").await;
        assert!(matches!(result, Err(SsrfError::Blocked(_))));
    }

    #[tokio::test]
    async fn check_ip_literal_public() {
        let result = SsrfGuard::check("http://8.8.8.8:8080/path").await;
        assert!(result.is_ok());
        let ips = result.expect("public IP check succeeded");
        assert_eq!(ips.len(), 1);
        assert_eq!(ips[0], IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));
    }

    /// SRC-011: A public IPv6 literal URL passes the SSRF guard and returns
    /// the parsed IP, enabling fetch of IPv6-literal subscription sources.
    #[tokio::test]
    async fn check_ip_literal_ipv6_public() {
        let result = SsrfGuard::check("https://[2606:4700:4700::1111]:443/sub").await;
        assert!(result.is_ok(), "public IPv6 literal should pass SSRF");
        let ips = result.expect("public IPv6 check succeeded");
        assert_eq!(ips.len(), 1);
        assert_eq!(
            ips[0],
            IpAddr::V6(
                "2606:4700:4700::1111"
                    .parse::<Ipv6Addr>()
                    .expect("valid IPv6")
            )
        );
    }

    #[tokio::test]
    async fn check_invalid_url() {
        assert!(matches!(
            SsrfGuard::check("not-a-url").await,
            Err(SsrfError::InvalidUrl(_))
        ));
    }

    #[tokio::test]
    async fn check_no_host() {
        assert!(matches!(
            SsrfGuard::check("file:///etc/passwd").await,
            Err(SsrfError::InvalidUrl(_))
        ));
    }
}
