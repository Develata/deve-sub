//! In-memory `GeoIpPort` stub for tests.
//!
//! Maps host strings to pre-configured [`RegionDetection`] results. Returns
//! an empty detection for unmapped hosts.

use std::collections::HashMap;
use std::net::IpAddr;

use async_trait::async_trait;

use deve_sub_application::source::geoip::{GeoIpPort, RegionDetection};

/// Stub GeoIP lookup that returns pre-configured results per host.
pub struct InMemoryGeoIp {
    mappings: HashMap<String, RegionDetection>,
}

impl InMemoryGeoIp {
    #[must_use]
    pub fn new() -> Self {
        Self {
            mappings: HashMap::new(),
        }
    }

    /// Register a host → (region, candidate_ips) mapping.
    #[must_use]
    pub fn with_mapping(mut self, host: &str, region: Option<String>, ips: Vec<IpAddr>) -> Self {
        self.mappings.insert(
            host.to_owned(),
            RegionDetection {
                region,
                candidate_ips: ips,
            },
        );
        self
    }
}

impl Default for InMemoryGeoIp {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GeoIpPort for InMemoryGeoIp {
    async fn detect_region(&self, host: &str) -> RegionDetection {
        self.mappings.get(host).cloned().unwrap_or(RegionDetection {
            region: None,
            candidate_ips: Vec::new(),
        })
    }
}
