//! GeoIP region detection port and enrichment logic.
//!
//! [`GeoIpPort`] is an infrastructure port defined in the application layer
//! and implemented by adapters (e.g. `MaxMindGeoIp` in `deve-sub-adapters`).
//! The [`enrich_regions`] function is called before reconcile to auto-detect
//! regions for newly-parsed nodes (NODE-007/008/009).

use std::net::IpAddr;

use async_trait::async_trait;
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};

use deve_sub_domain::{ReconcileEntry, RegionAssignment, RegionMethod};

/// Result of a GeoIP region detection for a single host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionDetection {
    /// ISO region code or free-form label, or `None` if lookup failed.
    pub region: Option<String>,
    /// All resolved candidate IPs (NODE-009: dual-stack domains record both
    /// IPv4 and IPv6 addresses). Implementations must return them sorted and
    /// deduplicated so stored data and region selection are deterministic
    /// across refreshes.
    pub candidate_ips: Vec<IpAddr>,
}

/// Infrastructure port for GeoIP-based region detection.
///
/// Implementations resolve a host (IP literal or domain name) to IP
/// addresses and look up the region via a GeoIP database. If the database
/// is unavailable, `detect_region` returns `region: None` but still
/// populates `candidate_ips` (graceful degradation).
#[async_trait]
pub trait GeoIpPort: Send + Sync {
    /// Detect the region for a host. Returns the region code and all
    /// resolved candidate IPs.
    async fn detect_region(&self, host: &str) -> RegionDetection;
}

/// Enrich parsed entries with auto-detected regions before reconcile
/// (NODE-007/008/009).
///
/// For each entry with a parsed node, calls [`GeoIpPort::detect_region`] on
/// the node's endpoint host. Sets `node.region` to `Auto` with the detected
/// value, and records candidate IPs in `node.extras["candidate_ips"]` as a
/// JSON array of strings.
///
/// Lookups run concurrently with a bounded [`tokio::sync::Semaphore`] (8 by
/// default) to avoid hammering DNS/resolver when a source has hundreds of
/// nodes (W-FF). Mutations are applied in entry order after all lookups
/// resolve so the output order matches the input.
pub async fn enrich_regions(entries: &mut [ReconcileEntry], geoip: &dyn GeoIpPort) {
    const MAX_CONCURRENT_LOOKUPS: usize = 8;

    let hosts: Vec<Option<String>> = entries
        .iter()
        .map(|e| {
            e.node
                .as_ref()
                .map(|n| n.endpoint.host.uri_host().to_owned())
        })
        .collect();

    // WHY: dedupe hosts before lookup so nodes sharing a host (common in
    // subscription lists: same server, multiple ports/protocols) share one
    // DNS/GeoIP resolution instead of re-resolving per entry. See R3-21.
    let unique_hosts: Vec<String> = {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        hosts
            .iter()
            .filter_map(|h| h.clone())
            .filter(|h| seen.insert(h.clone()))
            .collect()
    };

    let host_lookups = unique_hosts.into_iter().map(|host| async move {
        let detection = geoip.detect_region(&host).await;
        (host, detection)
    });

    // WHY: `buffered` polls up to N futures concurrently without spawning,
    // so borrowed (non-'static) futures that capture `geoip` by reference
    // work here. Results come back in input order, matching entry order.
    let host_results: Vec<(String, RegionDetection)> = stream::iter(host_lookups)
        .buffered(MAX_CONCURRENT_LOOKUPS)
        .collect()
        .await;

    let detection_by_host: std::collections::HashMap<&str, &RegionDetection> =
        host_results.iter().map(|(h, d)| (h.as_str(), d)).collect();

    for (entry, host_opt) in entries.iter_mut().zip(hosts) {
        if let (Some(node), Some(host)) = (&mut entry.node, host_opt)
            && let Some(detection) = detection_by_host.get(host.as_str())
        {
            node.region = RegionAssignment {
                method: RegionMethod::Auto,
                value: detection.region.clone(),
            };
            let ip_strings: Vec<String> = detection
                .candidate_ips
                .iter()
                .map(|ip| ip.to_string())
                .collect();
            if !ip_strings.is_empty() {
                node.extras.insert(
                    "candidate_ips".to_owned(),
                    serde_json::to_value(&ip_strings).unwrap_or_default(),
                );
            }
        }
    }
}
