//! Source-level include/exclude filter (SRC-010).
//!
//! Filtering is split into two phases to respect the GeoIP enrichment
//! ordering:
//!
//! 1. **Protocol filter** ([`apply_protocol_filter`]) — runs after parse,
//!    before region enrichment. Drops entries by protocol so filtered nodes
//!    do not consume GeoIP lookups.
//! 2. **Region filter** ([`apply_region_filter`]) — runs after region
//!    enrichment, before reconcile. At this point `node.region.value`
//!    reflects the GeoIP-detected (or pre-parsed) region, so region rules
//!    match against real region values.
//!
//! Entries that do not match the include rules or that match the exclude
//! rules are dropped: `node` set to `None` and `initial_status` set to
//! [`ItemParseStatus::Filtered`].

use deve_sub_domain::{ItemParseStatus, ReconcileEntry, SourceFilterRules};

/// Apply protocol include/exclude rules to `entries` in place (phase 1).
///
/// Runs before GeoIP enrichment so filtered nodes skip the DNS + GeoIP
/// lookup cost. An entry is filtered when its protocol is not in
/// `include_protocols` (when non-empty) or is in `exclude_protocols`.
/// Comparison is case-insensitive against
/// [`ProtocolKind::as_filter_key`](deve_sub_domain::ProtocolKind::as_filter_key).
pub fn apply_protocol_filter(entries: &mut [ReconcileEntry], rules: &SourceFilterRules) {
    for entry in entries.iter_mut() {
        let Some(node) = entry.node.as_ref() else {
            continue;
        };
        let protocol = node.protocol.as_filter_key();
        if is_protocol_filtered(protocol, rules) {
            entry.node = None;
            entry.initial_status = ItemParseStatus::Filtered;
        }
    }
}

/// Apply region include/exclude rules to `entries` in place (phase 2).
///
/// Runs after GeoIP enrichment so `node.region.value` reflects the
/// auto-detected or pre-parsed region. An entry is filtered when its region
/// is not in `include_regions` (when non-empty) or is in `exclude_regions`.
/// Comparison is case-insensitive. Entries with `region = None` (GeoIP
/// lookup failed) are treated as empty string `""`.
pub fn apply_region_filter(entries: &mut [ReconcileEntry], rules: &SourceFilterRules) {
    if rules.include_regions.is_empty() && rules.exclude_regions.is_empty() {
        return;
    }
    for entry in entries.iter_mut() {
        let Some(node) = entry.node.as_ref() else {
            continue;
        };
        let region = node.region.value.as_deref().unwrap_or("");
        if is_region_filtered(region, rules) {
            entry.node = None;
            entry.initial_status = ItemParseStatus::Filtered;
        }
    }
}

fn is_protocol_filtered(protocol: &str, rules: &SourceFilterRules) -> bool {
    if !rules.include_protocols.is_empty()
        && !rules
            .include_protocols
            .iter()
            .any(|p| p.eq_ignore_ascii_case(protocol))
    {
        return true;
    }
    if rules
        .exclude_protocols
        .iter()
        .any(|p| p.eq_ignore_ascii_case(protocol))
    {
        return true;
    }
    false
}

fn is_region_filtered(region: &str, rules: &SourceFilterRules) -> bool {
    if !rules.include_regions.is_empty()
        && !rules
            .include_regions
            .iter()
            .any(|r| r.eq_ignore_ascii_case(region))
    {
        return true;
    }
    if rules
        .exclude_regions
        .iter()
        .any(|r| r.eq_ignore_ascii_case(region))
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use deve_sub_domain::{
        Authentication, DomainName, Endpoint, Host, Node, NodeSource, ProtocolConfig, ProtocolKind,
        RegionAssignment, RegionMethod, TrojanConfig, UdpCapability, VMessConfig,
    };
    use deve_sub_kernel::{NodeId, Timestamp};

    use super::*;

    fn make_node(protocol: ProtocolKind, region: Option<&str>) -> ReconcileEntry {
        let config = match protocol {
            ProtocolKind::Trojan => ProtocolConfig::Trojan(TrojanConfig {
                packet_encoding: None,
            }),
            ProtocolKind::VMess => ProtocolConfig::VMess(VMessConfig {
                alter_id: None,
                security: None,
                packet_encoding: None,
            }),
            _ => unreachable!("test only uses Trojan/VMess"),
        };
        let node = Node {
            id: NodeId::new(),
            display_name: "test".to_owned(),
            protocol,
            config,
            endpoint: Endpoint {
                host: Host::Domain(DomainName::new("example.com".to_owned())),
                port: 443,
            },
            authentication: Authentication::Password {
                password: "TEST".to_owned(),
            },
            transport: None,
            tls: None,
            udp: UdpCapability {
                supported: None,
                xudp: None,
            },
            multiplex: None,
            obfuscation: None,
            congestion: None,
            chain: None,
            source: NodeSource {
                source_label: "test".to_owned(),
                raw_uri: None,
                imported_at: Timestamp::now(),
            },
            tags: vec![],
            region: RegionAssignment {
                method: RegionMethod::Auto,
                value: region.map(str::to_owned),
            },
            extras: BTreeMap::new(),
        };
        ReconcileEntry {
            raw_uri: "trojan://TEST@example.com:443".to_owned(),
            initial_status: ItemParseStatus::Parsed,
            node: Some(node),
        }
    }

    #[test]
    fn no_rules_keeps_all() {
        let mut entries = vec![
            make_node(ProtocolKind::Trojan, Some("US")),
            make_node(ProtocolKind::VMess, Some("CN")),
        ];
        let rules = SourceFilterRules::default();
        apply_protocol_filter(&mut entries, &rules);
        apply_region_filter(&mut entries, &rules);
        assert!(entries[0].node.is_some());
        assert!(entries[1].node.is_some());
    }

    #[test]
    fn include_protocols_keeps_only_matching() {
        let mut entries = vec![
            make_node(ProtocolKind::Trojan, None),
            make_node(ProtocolKind::VMess, None),
        ];
        let rules = SourceFilterRules {
            include_protocols: vec!["trojan".to_owned()],
            ..Default::default()
        };
        apply_protocol_filter(&mut entries, &rules);
        assert!(entries[0].node.is_some(), "trojan kept");
        assert!(entries[1].node.is_none(), "vmess filtered");
        assert_eq!(entries[1].initial_status, ItemParseStatus::Filtered);
    }

    #[test]
    fn exclude_protocols_drops_matching() {
        let mut entries = vec![
            make_node(ProtocolKind::Trojan, None),
            make_node(ProtocolKind::VMess, None),
        ];
        let rules = SourceFilterRules {
            exclude_protocols: vec!["trojan".to_owned()],
            ..Default::default()
        };
        apply_protocol_filter(&mut entries, &rules);
        assert!(entries[0].node.is_none(), "trojan excluded");
        assert!(entries[1].node.is_some(), "vmess kept");
    }

    #[test]
    fn include_regions_keeps_only_matching() {
        let mut entries = vec![
            make_node(ProtocolKind::Trojan, Some("US")),
            make_node(ProtocolKind::Trojan, Some("CN")),
        ];
        let rules = SourceFilterRules {
            include_regions: vec!["us".to_owned()],
            ..Default::default()
        };
        apply_region_filter(&mut entries, &rules);
        assert!(entries[0].node.is_some(), "US kept");
        assert!(entries[1].node.is_none(), "CN filtered");
    }

    #[test]
    fn exclude_regions_drops_matching() {
        let mut entries = vec![
            make_node(ProtocolKind::Trojan, Some("US")),
            make_node(ProtocolKind::Trojan, Some("CN")),
        ];
        let rules = SourceFilterRules {
            exclude_regions: vec!["cn".to_owned()],
            ..Default::default()
        };
        apply_region_filter(&mut entries, &rules);
        assert!(entries[0].node.is_some(), "US kept");
        assert!(entries[1].node.is_none(), "CN excluded");
    }

    #[test]
    fn protocol_match_is_case_insensitive() {
        let mut entries = vec![make_node(ProtocolKind::Trojan, None)];
        let rules = SourceFilterRules {
            include_protocols: vec!["TROJAN".to_owned()],
            ..Default::default()
        };
        apply_protocol_filter(&mut entries, &rules);
        assert!(
            entries[0].node.is_some(),
            "case-insensitive match keeps node"
        );
    }

    #[test]
    fn entries_without_node_are_skipped() {
        let mut entries = vec![ReconcileEntry {
            raw_uri: "bad-uri".to_owned(),
            initial_status: ItemParseStatus::Failed,
            node: None,
        }];
        let rules = SourceFilterRules {
            include_protocols: vec!["trojan".to_owned()],
            ..Default::default()
        };
        apply_protocol_filter(&mut entries, &rules);
        assert!(entries[0].node.is_none());
        assert_eq!(entries[0].initial_status, ItemParseStatus::Failed);
    }

    /// SRC-010 integration: protocol filter runs first (before region is
    /// known), region filter runs second (after GeoIP enrichment). A node
    /// with no pre-set region survives the protocol phase but can be
    /// filtered by the region phase once its region is assigned.
    #[test]
    fn two_phase_pipeline_protocol_then_region() {
        let mut entries = vec![
            make_node(ProtocolKind::Trojan, None),
            make_node(ProtocolKind::VMess, None),
        ];
        let rules = SourceFilterRules {
            include_protocols: vec!["trojan".to_owned()],
            include_regions: vec!["us".to_owned()],
            ..Default::default()
        };

        apply_protocol_filter(&mut entries, &rules);
        assert!(entries[0].node.is_some(), "trojan survives protocol phase");
        assert!(
            entries[1].node.is_none(),
            "vmess filtered in protocol phase"
        );

        // Simulate GeoIP enrichment: assign region to the surviving entry.
        if let Some(node) = entries[0].node.as_mut() {
            node.region = RegionAssignment {
                method: RegionMethod::Auto,
                value: Some("US".to_owned()),
            };
        }
        apply_region_filter(&mut entries, &rules);
        assert!(
            entries[0].node.is_some(),
            "US-region trojan survives region phase"
        );
    }

    /// SRC-010 regression: region filter on pre-enrichment entries (region
    /// = None) must NOT drop everything when include_regions is set. The
    /// region phase is a no-op when no region rules are configured.
    #[test]
    fn region_phase_noop_when_no_region_rules() {
        let mut entries = vec![make_node(ProtocolKind::Trojan, None)];
        let rules = SourceFilterRules {
            include_protocols: vec!["trojan".to_owned()],
            ..Default::default()
        };
        apply_protocol_filter(&mut entries, &rules);
        apply_region_filter(&mut entries, &rules);
        assert!(
            entries[0].node.is_some(),
            "no region rules means no region filtering"
        );
    }
}
