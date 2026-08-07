//! MaxMind GeoIP adapter implementing [`GeoIpPort`].
//!
//! Uses the `maxminddb` crate to look up ISO country codes for node endpoint
//! hosts. Graceful degradation: when the `.mmdb` file is missing or unreadable,
//! `detect_region` still resolves and returns candidate IPs but yields
//! `region: None` (NODE-009 dual-stack candidate recording). GeoIP never
//! applies SSRF filtering — node hosts are proxy endpoints, not source URLs.

use std::net::IpAddr;

use async_trait::async_trait;
use deve_sub_application::{GeoIpPort, source::RegionDetection};
use tracing::warn;

/// MaxMind GeoIP adapter backed by a `.mmdb` country database.
///
/// Stores a [`maxminddb::Reader`] when a valid database file is supplied at
/// construction. A readerless adapter is the degraded state: lookups still
/// resolve and return candidate IPs but never produce a region.
pub struct MaxMindGeoIp {
    reader: Option<maxminddb::Reader<Vec<u8>>>,
}

impl MaxMindGeoIp {
    /// Create a new adapter from an optional `.mmdb` path.
    ///
    /// `None` or an unreadable path yields a readerless adapter (graceful
    /// degradation); the failure is logged once at construction time.
    #[must_use]
    pub fn new(mmdb_path: Option<&str>) -> Self {
        let reader = mmdb_path.and_then(|path| match maxminddb::Reader::open_readfile(path) {
            Ok(r) => Some(r),
            Err(e) => {
                warn!(
                    path = path, error = %e,
                    "failed to open GeoIP database; region detection disabled"
                );
                None
            }
        });
        Self { reader }
    }

    /// Resolve a domain host to candidate IPs via `tokio::net::lookup_host`.
    /// WHY: a domain may resolve to multiple A/AAAA records; collecting all
    /// of them satisfies NODE-009 dual-stack recording.
    async fn resolve_host(host: &str) -> Result<Vec<IpAddr>, std::io::Error> {
        let addrs: Vec<IpAddr> = tokio::net::lookup_host((host, 0u16))
            .await?
            .map(|sa| sa.ip())
            .collect();
        Ok(addrs)
    }

    /// Look up the ISO country code for a single IP. Returns `Ok(None)` when
    /// the IP has no country record; `Err` on a database decode failure.
    fn lookup_country(
        reader: &maxminddb::Reader<Vec<u8>>,
        ip: IpAddr,
    ) -> Result<Option<String>, maxminddb::MaxMindDbError> {
        let result = reader.lookup(ip)?;
        let Some(record) = result.decode::<maxminddb::geoip2::Country>()? else {
            return Ok(None);
        };
        Ok(record.country.iso_code.map(str::to_owned))
    }
}

#[async_trait]
impl GeoIpPort for MaxMindGeoIp {
    async fn detect_region(&self, host: &str) -> RegionDetection {
        // WHY: an IP literal needs no DNS resolution; a domain is resolved
        // via tokio so both IPv4 and IPv6 records are collected (NODE-009).
        let candidate_ips = match host.parse::<IpAddr>() {
            Ok(ip) => vec![ip],
            Err(_) => match Self::resolve_host(host).await {
                Ok(ips) => ips,
                Err(e) => {
                    warn!(
                        host = host, error = %e,
                        "DNS resolution failed; skipping region detection"
                    );
                    return RegionDetection {
                        region: None,
                        candidate_ips: vec![],
                    };
                }
            },
        };

        let Some(reader) = &self.reader else {
            // WHY: degraded mode — no database, but the caller still gets the
            // resolved candidate IPs for NODE-009 recording.
            return RegionDetection {
                region: None,
                candidate_ips,
            };
        };

        for ip in &candidate_ips {
            match Self::lookup_country(reader, *ip) {
                Ok(Some(code)) => {
                    return RegionDetection {
                        region: Some(code),
                        candidate_ips,
                    };
                }
                Ok(None) => continue,
                Err(e) => {
                    warn!(ip = %ip, error = %e, "GeoIP lookup failed for candidate IP; skipping");
                }
            }
        }

        RegionDetection {
            region: None,
            candidate_ips,
        }
    }
}
