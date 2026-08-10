//! Tests for `selection.rs` — extracted to keep the production source under
//! the 500-line fuse (F5.1). Included via `#[path]` so `use super::*` still
//! reaches the private helpers (`matches_all_filters`, `matches_filter_rule`,
//! `matches_quick_group`).

use super::*;
use deve_sub_domain::source::SourceError;
use deve_sub_domain::{Node, NodeSource, RegionAssignment, RegionMethod};
use deve_sub_kernel::{NodeId, Timestamp};

/// Build a minimal `NodePoolEntry` for testing.
fn make_entry(
    id: &str,
    display_name: &str,
    protocol: deve_sub_domain::ProtocolKind,
    region: Option<&str>,
    is_active: bool,
    missing: bool,
    tags: Vec<&str>,
) -> NodePoolEntry {
    let node = Node {
        id: NodeId::parse(id).expect("id"),
        display_name: display_name.to_owned(),
        protocol,
        config: deve_sub_domain::ProtocolConfig::Trojan(deve_sub_domain::TrojanConfig {
            packet_encoding: None,
        }),
        endpoint: deve_sub_domain::Endpoint {
            host: deve_sub_domain::Host::Domain(deve_sub_domain::DomainName::new(
                "example.com".to_owned(),
            )),
            port: 443,
        },
        authentication: deve_sub_domain::Authentication::Password {
            password: "x".to_owned(),
        },
        transport: None,
        tls: None,
        udp: deve_sub_domain::UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "test-source".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::from_unix_ms(0).expect("ts"),
        },
        tags: Vec::new(),
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: region.map(str::to_owned),
        },
        extras: std::collections::BTreeMap::new(),
    };
    NodePoolEntry {
        node,
        missing_from_source: missing,
        is_active,
        revision: 1,
        created_at: Timestamp::from_unix_ms(0).expect("ts"),
        override_info: None,
        tags: tags
            .iter()
            .map(|name| deve_sub_domain::Tag {
                id: deve_sub_kernel::TagId::new(),
                name: (*name).to_owned(),
                color: None,
            })
            .collect(),
    }
}

#[test]
fn matches_filter_rule_protocol_case_insensitive() {
    let entry = make_entry(
        "01KZGGGGGGGGGGGGGGGGGGGGGG",
        "node-1",
        deve_sub_domain::ProtocolKind::Trojan,
        None,
        true,
        false,
        vec![],
    );
    let rule = NodeFilterRule {
        field: FilterField::Protocol,
        value: "TROJAN".to_owned(),
    };
    assert!(matches_filter_rule(&entry, &rule));
}

#[test]
fn matches_filter_rule_region() {
    let entry = make_entry(
        "01KZGGGGGGGGGGGGGGGGGGGGGG",
        "node-1",
        deve_sub_domain::ProtocolKind::Trojan,
        Some("US"),
        true,
        false,
        vec![],
    );
    let rule = NodeFilterRule {
        field: FilterField::Region,
        value: "us".to_owned(),
    };
    assert!(matches_filter_rule(&entry, &rule));
}

#[test]
fn matches_filter_rule_tag() {
    let entry = make_entry(
        "01KZGGGGGGGGGGGGGGGGGGGGGG",
        "node-1",
        deve_sub_domain::ProtocolKind::Trojan,
        None,
        true,
        false,
        vec!["production"],
    );
    let rule = NodeFilterRule {
        field: FilterField::Tag,
        value: "PRODUCTION".to_owned(),
    };
    assert!(matches_filter_rule(&entry, &rule));
}

#[test]
fn matches_quick_group_all_criteria() {
    let entry = make_entry(
        "01KZGGGGGGGGGGGGGGGGGGGGGG",
        "node-1",
        deve_sub_domain::ProtocolKind::Trojan,
        Some("US"),
        true,
        false,
        vec!["production"],
    );
    let filter = QuickGroupFilter {
        region: Some("US".to_owned()),
        protocol: Some("trojan".to_owned()),
        tag: Some("production".to_owned()),
    };
    assert!(matches_quick_group(&entry, &filter));
}

#[test]
fn matches_quick_group_partial_mismatch() {
    let entry = make_entry(
        "01KZGGGGGGGGGGGGGGGGGGGGGG",
        "node-1",
        deve_sub_domain::ProtocolKind::Vless,
        Some("US"),
        true,
        false,
        vec![],
    );
    let filter = QuickGroupFilter {
        region: Some("US".to_owned()),
        protocol: Some("trojan".to_owned()),
        tag: None,
    };
    assert!(!matches_quick_group(&entry, &filter));
}

#[test]
fn apply_sort_order_ascending() {
    let id_a = NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAAAA").expect("valid ULID");
    let id_b = NodeId::parse("01KZBBBBBBBBBBBBBBBBBBBBBB").expect("valid ULID");
    let id_c = NodeId::parse("01KZCCCCCCCCCCCCCCCCCCCCCC").expect("valid ULID");
    let entries = vec![
        make_entry(
            "01KZCCCCCCCCCCCCCCCCCCCCCC",
            "charlie",
            deve_sub_domain::ProtocolKind::Trojan,
            None,
            true,
            false,
            vec![],
        ),
        make_entry(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            "alpha",
            deve_sub_domain::ProtocolKind::Trojan,
            None,
            true,
            false,
            vec![],
        ),
        make_entry(
            "01KZBBBBBBBBBBBBBBBBBBBBBB",
            "bravo",
            deve_sub_domain::ProtocolKind::Trojan,
            None,
            true,
            false,
            vec![],
        ),
    ];
    let mut ids = vec![id_c, id_a, id_b];
    apply_sort_order(&mut ids, &entries, SortOrder::Asc);
    assert_eq!(ids, vec![id_a, id_b, id_c]);
}

#[test]
fn apply_sort_order_descending() {
    let id_a = NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAAAA").expect("valid ULID");
    let id_b = NodeId::parse("01KZBBBBBBBBBBBBBBBBBBBBBB").expect("valid ULID");
    let id_c = NodeId::parse("01KZCCCCCCCCCCCCCCCCCCCCCC").expect("valid ULID");
    let entries = vec![
        make_entry(
            "01KZCCCCCCCCCCCCCCCCCCCCCC",
            "charlie",
            deve_sub_domain::ProtocolKind::Trojan,
            None,
            true,
            false,
            vec![],
        ),
        make_entry(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            "alpha",
            deve_sub_domain::ProtocolKind::Trojan,
            None,
            true,
            false,
            vec![],
        ),
        make_entry(
            "01KZBBBBBBBBBBBBBBBBBBBBBB",
            "bravo",
            deve_sub_domain::ProtocolKind::Trojan,
            None,
            true,
            false,
            vec![],
        ),
    ];
    let mut ids = vec![id_a, id_b, id_c];
    apply_sort_order(&mut ids, &entries, SortOrder::Desc);
    assert_eq!(ids, vec![id_c, id_b, id_a]);
}

/// A mock pool repository that returns a fixed set of entries.
struct MockPool {
    entries: Vec<NodePoolEntry>,
}

#[async_trait::async_trait]
impl NodePoolRepository for MockPool {
    async fn reconcile(
        &self,
        _input: deve_sub_domain::source::ReconcileInput<'_>,
    ) -> Result<deve_sub_domain::source::ReconcileResult, SourceError> {
        unimplemented!("not needed for selection tests")
    }
    async fn list_nodes(
        &self,
        _filter: &NodeFilter,
        cursor: Option<NodeId>,
        limit: u32,
    ) -> Result<Vec<NodePoolEntry>, SourceError> {
        let start = match cursor {
            None => 0,
            Some(c) => self
                .entries
                .iter()
                .position(|e| e.node.id > c)
                .unwrap_or(self.entries.len()),
        };
        Ok(self
            .entries
            .iter()
            .skip(start)
            .take(limit as usize)
            .cloned()
            .collect())
    }
    async fn get_node(&self, id: NodeId) -> Result<Option<NodePoolEntry>, SourceError> {
        Ok(self.entries.iter().find(|e| e.node.id == id).cloned())
    }
    async fn import_nodes(
        &self,
        _nodes: Vec<Node>,
    ) -> Result<deve_sub_domain::source::ImportResult, SourceError> {
        unimplemented!("not needed for selection tests")
    }
    async fn list_node_chains(&self) -> Result<Vec<(NodeId, Vec<NodeId>)>, SourceError> {
        Ok(Vec::new())
    }
    async fn set_node_chain(
        &self,
        _node_id: NodeId,
        _chain: Option<&[NodeId]>,
    ) -> Result<(), SourceError> {
        unimplemented!("not needed for selection tests")
    }
}

#[tokio::test]
async fn resolve_selection_dynamic_filters_by_protocol() {
    let entries = vec![
        make_entry(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            "alpha",
            deve_sub_domain::ProtocolKind::Trojan,
            Some("US"),
            true,
            false,
            vec![],
        ),
        make_entry(
            "01KZBBBBBBBBBBBBBBBBBBBBBB",
            "bravo",
            deve_sub_domain::ProtocolKind::Vless,
            Some("JP"),
            true,
            false,
            vec![],
        ),
    ];
    let pool = MockPool { entries };
    let selector = NodeSelector {
        mode: SelectionMode::Dynamic,
        filters: vec![NodeFilterRule {
            field: FilterField::Protocol,
            value: "trojan".to_owned(),
        }],
        node_ids: vec![],
        node_revision: 0,
    };
    let (ids, missing) = resolve_selection(&selector, &pool).await.expect("resolve");
    assert_eq!(ids.len(), 1);
    assert!(ids[0].to_string().starts_with("01KZAA"));
    assert!(missing.is_empty());
}

#[tokio::test]
async fn resolve_selection_fixed_returns_only_pinned() {
    let entries = vec![
        make_entry(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            "alpha",
            deve_sub_domain::ProtocolKind::Trojan,
            None,
            true,
            false,
            vec![],
        ),
        make_entry(
            "01KZBBBBBBBBBBBBBBBBBBBBBB",
            "bravo",
            deve_sub_domain::ProtocolKind::Trojan,
            None,
            true,
            false,
            vec![],
        ),
    ];
    let pool = MockPool { entries };
    let selector = NodeSelector {
        mode: SelectionMode::Fixed,
        filters: vec![],
        node_ids: vec![NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAAAA").expect("valid ULID")],
        node_revision: 1,
    };
    let (ids, missing) = resolve_selection(&selector, &pool).await.expect("resolve");
    assert_eq!(ids.len(), 1);
    assert!(missing.is_empty());
}

#[tokio::test]
async fn resolve_selection_fixed_reports_missing() {
    let entries = vec![make_entry(
        "01KZAAAAAAAAAAAAAAAAAAAAAA",
        "alpha",
        deve_sub_domain::ProtocolKind::Trojan,
        None,
        true,
        false,
        vec![],
    )];
    let pool = MockPool { entries };
    let selector = NodeSelector {
        mode: SelectionMode::Fixed,
        filters: vec![],
        node_ids: vec![
            NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAAAA").expect("valid ULID"),
            NodeId::parse("01KZBBBBBBBBBBBBBBBBBBBBBB").expect("valid ULID"),
        ],
        node_revision: 1,
    };
    let (ids, missing) = resolve_selection(&selector, &pool).await.expect("resolve");
    assert_eq!(ids.len(), 1);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].reason, MissingReason::NotFound);
}

#[tokio::test]
async fn resolve_group_with_quick_group_filter() {
    let entries = vec![
        make_entry(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            "alpha",
            deve_sub_domain::ProtocolKind::Trojan,
            Some("US"),
            true,
            false,
            vec![],
        ),
        make_entry(
            "01KZBBBBBBBBBBBBBBBBBBBBBB",
            "bravo",
            deve_sub_domain::ProtocolKind::Vless,
            Some("US"),
            true,
            false,
            vec![],
        ),
        make_entry(
            "01KZCCCCCCCCCCCCCCCCCCCCCC",
            "charlie",
            deve_sub_domain::ProtocolKind::Trojan,
            Some("JP"),
            true,
            false,
            vec![],
        ),
    ];
    let pool = MockPool { entries };
    let group = ProxyGroup {
        name: "us-trojan".to_owned(),
        group_type: deve_sub_domain::template::GroupType::Select,
        members: vec![],
        filter: Some(QuickGroupFilter {
            region: Some("US".to_owned()),
            protocol: Some("trojan".to_owned()),
            tag: None,
        }),
        sort_order: None,
    };
    let resolution = resolve_group(&group, &pool).await.expect("resolve");
    assert_eq!(resolution.group_name, "us-trojan");
    assert!(resolution.explicit_node_ids.is_empty());
    assert_eq!(resolution.quick_group_node_ids.len(), 1);
    assert!(resolution.missing.is_empty());
}

#[tokio::test]
async fn resolve_group_reports_inactive_and_missing() {
    let entries = vec![
        make_entry(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            "alpha",
            deve_sub_domain::ProtocolKind::Trojan,
            None,
            false,
            false,
            vec![],
        ),
        make_entry(
            "01KZBBBBBBBBBBBBBBBBBBBBBB",
            "bravo",
            deve_sub_domain::ProtocolKind::Trojan,
            None,
            true,
            true,
            vec![],
        ),
    ];
    let pool = MockPool { entries };
    let group = ProxyGroup {
        name: "test".to_owned(),
        group_type: deve_sub_domain::template::GroupType::Select,
        members: vec![
            GroupMember::Node {
                id: NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAAAA").expect("valid ULID"),
            },
            GroupMember::Node {
                id: NodeId::parse("01KZBBBBBBBBBBBBBBBBBBBBBB").expect("valid ULID"),
            },
            GroupMember::Node {
                id: NodeId::parse("01KZCCCCCCCCCCCCCCCCCCCCCC").expect("valid ULID"),
            },
        ],
        filter: None,
        sort_order: None,
    };
    let resolution = resolve_group(&group, &pool).await.expect("resolve");
    assert!(resolution.explicit_node_ids.is_empty());
    assert_eq!(resolution.missing.len(), 3);
    assert_eq!(resolution.missing[0].reason, MissingReason::Inactive);
    assert_eq!(
        resolution.missing[1].reason,
        MissingReason::MissingFromSource
    );
    assert_eq!(resolution.missing[2].reason, MissingReason::NotFound);
}
