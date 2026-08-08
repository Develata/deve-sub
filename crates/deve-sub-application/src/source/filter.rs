//! Source-level include/exclude filter (SRC-010).
//!
//! Applied to parsed [`ReconcileEntry`]s after parse and before region
//! enrichment / reconcile. Entries that do not match the include rules or
//! that match the exclude rules are dropped: `node` set to `None` and
//! `initial_status` set to [`ItemParseStatus::Filtered`].

use deve_sub_domain::{ItemParseStatus, ReconcileEntry, SourceFilterRules};

/// Apply `rules` to `entries` in place.
///
/// An entry is filtered when its protocol is not in `include_protocols`
/// (when non-empty), is in `exclude_protocols`, its region is not in
/// `include_regions` (when non-empty), or is in `exclude_regions`. Protocol
/// and region comparisons are case-insensitive.
pub fn apply_source_filter(entries: &mut [ReconcileEntry], rules: &SourceFilterRules) {
    for entry in entries.iter_mut() {
        let Some(node) = entry.node.as_ref() else {
            continue;
        };
        let protocol = node.protocol.as_filter_key();
        let region = node.region.value.as_deref().unwrap_or("");
        if is_filtered(protocol, region, rules) {
            entry.node = None;
            entry.initial_status = ItemParseStatus::Filtered;
        }
    }
}

fn is_filtered(protocol: &str, region: &str, rules: &SourceFilterRules) -> bool {
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
        apply_source_filter(&mut entries, &rules);
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
        apply_source_filter(&mut entries, &rules);
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
        apply_source_filter(&mut entries, &rules);
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
        apply_source_filter(&mut entries, &rules);
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
        apply_source_filter(&mut entries, &rules);
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
        apply_source_filter(&mut entries, &rules);
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
        apply_source_filter(&mut entries, &rules);
        assert!(entries[0].node.is_none());
        assert_eq!(entries[0].initial_status, ItemParseStatus::Failed);
    }
}
