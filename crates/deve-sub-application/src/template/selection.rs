//! Node selection and proxy-group resolution against the live node pool.
//!
//! This module resolves a [`TemplateDocument`]'s `nodeSelector` and
//! `proxyGroups` into concrete node IDs by querying the
//! [`NodePoolRepository`]. It handles:
//!
//! - **Dynamic selection**: apply `NodeFilterRule`s to all active, non-missing
//!   pool entries. New nodes that match the filters are automatically included
//!   (GEN-005).
//! - **Fixed selection**: look up pinned `node_ids` individually. New nodes are
//!   not included because they are not in the pinned list (GEN-006).
//! - **Quick-group filters**: auto-populate group members by region, protocol,
//!   or tag (GEN-007, GEN-008).
//! - **Missing reference detection**: report node references that are not
//!   found, missing from source, or inactive (GEN-011).
//! - **Sort order**: apply ascending/descending alphabetical sort to resolved
//!   members.
//!
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"Proxy group
//! model" and §"Generation pipeline".

use deve_sub_domain::source::{NodeFilter, NodePoolEntry, NodePoolRepository};
use deve_sub_domain::template::{
    FilterField, GroupMember, GroupResolution, MissingNodeRef, MissingReason, NodeFilterRule,
    NodeSelector, ProxyGroup, QuickGroupFilter, SelectionMode, SortOrder, TemplateDocument,
    TemplateResolution,
};
use deve_sub_kernel::NodeId;

use super::error::TemplateAppError;

/// Page size for pool listing. Large enough to cover typical deployments in
/// one or two pages; the loop continues until exhausted.
const POOL_PAGE_SIZE: u32 = 1000;

/// Resolve a template's node selector and all proxy groups against the live
/// pool.
///
/// This is a read-only operation: it queries the pool and returns which nodes
/// are selected, which group members are resolved, and which references are
/// missing. It does not generate output or modify state.
pub async fn resolve_template(
    doc: &TemplateDocument,
    pool_repo: &dyn NodePoolRepository,
) -> Result<TemplateResolution, TemplateAppError> {
    let (selected_node_ids, selection_missing) =
        resolve_selection(&doc.spec.node_selector, pool_repo).await?;

    let mut groups = Vec::with_capacity(doc.spec.proxy_groups.len());
    for group in &doc.spec.proxy_groups {
        let resolution = resolve_group(group, pool_repo).await?;
        groups.push(resolution);
    }

    Ok(TemplateResolution {
        selected_node_ids,
        selection_missing,
        groups,
    })
}

/// Resolve the template's `nodeSelector` against the pool.
///
/// - **Dynamic**: list all active, non-missing nodes and apply filter rules.
///   Matching node IDs are returned in pool order (by `NodeId`).
/// - **Fixed**: look up each pinned `node_id` individually. Found and active
///   nodes are returned; missing ones are reported.
pub async fn resolve_selection(
    selector: &NodeSelector,
    pool_repo: &dyn NodePoolRepository,
) -> Result<(Vec<NodeId>, Vec<MissingNodeRef>), TemplateAppError> {
    match selector.mode {
        SelectionMode::Dynamic => {
            let entries = list_active_nodes(pool_repo).await?;
            let filtered: Vec<NodeId> = entries
                .iter()
                .filter(|e| matches_all_filters(e, &selector.filters))
                .map(|e| e.node.id)
                .collect();
            Ok((filtered, Vec::new()))
        }
        SelectionMode::Fixed => {
            let mut found = Vec::with_capacity(selector.node_ids.len());
            let mut missing = Vec::new();
            for id in &selector.node_ids {
                match pool_repo.get_node(*id).await {
                    Ok(Some(entry)) if entry.is_active && !entry.missing_from_source => {
                        found.push(*id);
                    }
                    Ok(Some(entry)) if entry.missing_from_source => {
                        missing.push(MissingNodeRef {
                            node_id: *id,
                            reason: MissingReason::MissingFromSource,
                        });
                    }
                    Ok(Some(_entry)) => {
                        missing.push(MissingNodeRef {
                            node_id: *id,
                            reason: MissingReason::Inactive,
                        });
                    }
                    Ok(None) => {
                        missing.push(MissingNodeRef {
                            node_id: *id,
                            reason: MissingReason::NotFound,
                        });
                    }
                    Err(e) => return Err(TemplateAppError::Storage(e.to_string())),
                }
            }
            Ok((found, missing))
        }
    }
}

/// Resolve a single proxy group's membership against the pool.
///
/// Explicit `GroupMember::Node` entries are checked individually. If a
/// `QuickGroupFilter` is present, matching nodes from the pool are appended
/// (deduplicated against explicit members). `GroupMember::Group` references are
/// not resolved here — they are validated structurally in `validate_document`.
pub async fn resolve_group(
    group: &ProxyGroup,
    pool_repo: &dyn NodePoolRepository,
) -> Result<GroupResolution, TemplateAppError> {
    let mut explicit_node_ids = Vec::new();
    let mut missing = Vec::new();
    let mut explicit_set: std::collections::HashSet<NodeId> = std::collections::HashSet::new();

    for member in &group.members {
        if let GroupMember::Node { id } = member {
            match pool_repo.get_node(*id).await {
                Ok(Some(entry)) if entry.is_active && !entry.missing_from_source => {
                    explicit_node_ids.push(*id);
                    explicit_set.insert(*id);
                }
                Ok(Some(entry)) if entry.missing_from_source => {
                    missing.push(MissingNodeRef {
                        node_id: *id,
                        reason: MissingReason::MissingFromSource,
                    });
                }
                Ok(Some(_)) => {
                    missing.push(MissingNodeRef {
                        node_id: *id,
                        reason: MissingReason::Inactive,
                    });
                }
                Ok(None) => {
                    missing.push(MissingNodeRef {
                        node_id: *id,
                        reason: MissingReason::NotFound,
                    });
                }
                Err(e) => return Err(TemplateAppError::Storage(e.to_string())),
            }
        }
    }

    let mut quick_group_node_ids = Vec::new();
    if let Some(filter) = &group.filter {
        let entries = list_active_nodes(pool_repo).await?;
        for entry in &entries {
            if explicit_set.contains(&entry.node.id) {
                continue;
            }
            if matches_quick_group(entry, filter) {
                quick_group_node_ids.push(entry.node.id);
            }
        }
    }

    Ok(GroupResolution {
        group_name: group.name.clone(),
        explicit_node_ids,
        quick_group_node_ids,
        missing,
    })
}

/// Apply sort order to a list of node IDs using the display names from the
/// pool entries.
///
/// `SortOrder::Latency` is not yet supported (latency data arrives with
/// url-test probes in a later slice); it falls back to ascending alphabetical
/// order for now.
pub fn apply_sort_order(
    node_ids: &mut [NodeId],
    entries: &[NodePoolEntry],
    sort_order: SortOrder,
) {
    let name_by_id: std::collections::HashMap<NodeId, &str> = entries
        .iter()
        .map(|e| (e.node.id, e.node.display_name.as_str()))
        .collect();

    node_ids.sort_by(|a, b| {
        let name_a = name_by_id.get(a).copied().unwrap_or("");
        let name_b = name_by_id.get(b).copied().unwrap_or("");
        match sort_order {
            SortOrder::Asc | SortOrder::Latency => name_a.cmp(name_b),
            SortOrder::Desc => name_b.cmp(name_a),
        }
    });
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// List all active, non-missing nodes from the pool, paginating until
/// exhausted.
async fn list_active_nodes(
    pool_repo: &dyn NodePoolRepository,
) -> Result<Vec<NodePoolEntry>, TemplateAppError> {
    let filter = NodeFilter::active_only();
    let mut all = Vec::new();
    let mut cursor: Option<NodeId> = None;
    loop {
        let page = pool_repo
            .list_nodes(&filter, cursor, POOL_PAGE_SIZE)
            .await
            .map_err(|e| TemplateAppError::Storage(e.to_string()))?;
        if page.is_empty() {
            break;
        }
        cursor = page.last().map(|e| e.node.id);
        all.extend(page);
        if cursor.is_none() {
            break;
        }
    }
    Ok(all)
}

/// Check whether a pool entry matches all filter rules (AND semantics).
fn matches_all_filters(entry: &NodePoolEntry, rules: &[NodeFilterRule]) -> bool {
    rules.iter().all(|r| matches_filter_rule(entry, r))
}

/// Check whether a pool entry matches a single filter rule.
fn matches_filter_rule(entry: &NodePoolEntry, rule: &NodeFilterRule) -> bool {
    match rule.field {
        FilterField::Protocol => entry
            .node
            .protocol
            .as_filter_key()
            .eq_ignore_ascii_case(&rule.value),
        FilterField::Region => entry
            .node
            .region
            .value
            .as_deref()
            .unwrap_or("")
            .eq_ignore_ascii_case(&rule.value),
        FilterField::Tag => entry
            .tags
            .iter()
            .any(|t| t.name.eq_ignore_ascii_case(&rule.value)),
        FilterField::Source => entry
            .node
            .source
            .source_label
            .eq_ignore_ascii_case(&rule.value),
    }
}

/// Check whether a pool entry matches a quick-group filter (all set fields
/// must match, AND semantics).
fn matches_quick_group(entry: &NodePoolEntry, filter: &QuickGroupFilter) -> bool {
    if let Some(region) = &filter.region
        && !entry
            .node
            .region
            .value
            .as_deref()
            .unwrap_or("")
            .eq_ignore_ascii_case(region)
    {
        return false;
    }
    if let Some(protocol) = &filter.protocol
        && !entry
            .node
            .protocol
            .as_filter_key()
            .eq_ignore_ascii_case(protocol)
    {
        return false;
    }
    if let Some(tag) = &filter.tag
        && !entry.tags.iter().any(|t| t.name.eq_ignore_ascii_case(tag))
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
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
}
