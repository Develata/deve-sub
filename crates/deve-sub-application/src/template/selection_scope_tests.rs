//! Selector scope regressions: fixed membership must not read the entire pool.

use super::*;

fn entry(index: u8, active: bool, missing: bool) -> NodePoolEntry {
    make_entry(
        &format!("01KZAAAAAAAAAAAAAAAAAAAA{index:02}"),
        &format!("node-{index}"),
        deve_sub_domain::ProtocolKind::Trojan,
        Some("US"),
        active,
        missing,
        vec!["production"],
    )
}

fn document(selector: NodeSelector, group: ProxyGroup) -> TemplateDocument {
    let mut doc: TemplateDocument = serde_yaml::from_str(
        "apiVersion: deve-sub.io/v1\nkind: SubscriptionTemplate\nmetadata:\n  name: scope\nspec: {}",
    )
    .expect("valid document");
    doc.spec.node_selector = selector;
    doc.spec.proxy_groups = vec![group];
    doc
}

#[tokio::test]
async fn fixed_groups_reuse_pinned_nodes_and_preserve_reference_diagnostics() {
    let pool = MockPool {
        entries: vec![
            entry(1, true, false),
            entry(2, true, false),
            entry(3, true, false),
            entry(4, false, false),
            entry(5, true, true),
        ],
        ..Default::default()
    };
    let ids: Vec<_> = (1..=6).map(|i| entry(i, true, false).node.id).collect();
    let doc = document(
        NodeSelector {
            mode: SelectionMode::Fixed,
            // Pin order is preserved in selection, while quick groups follow
            // pool ID order and include each eligible node only once.
            node_ids: vec![ids[2], ids[0], ids[2], ids[3], ids[4], ids[5]],
            ..Default::default()
        },
        ProxyGroup {
            name: "selected-us".into(),
            group_type: deve_sub_domain::GroupType::Select,
            members: vec![GroupMember::Node { id: ids[1] }],
            filter: Some(QuickGroupFilter {
                region: Some("US".into()),
                protocol: Some("trojan".into()),
                tag: Some("production".into()),
            }),
            sort_order: None,
        },
    );
    let resolution = resolve_template(&doc, &pool).await.expect("resolve");
    assert_eq!(resolution.selected_node_ids, vec![ids[2], ids[0], ids[2]]);
    assert_eq!(
        resolution
            .selection_missing
            .iter()
            .map(|m| m.reason)
            .collect::<Vec<_>>(),
        vec![
            MissingReason::Inactive,
            MissingReason::MissingFromSource,
            MissingReason::NotFound
        ]
    );
    let group = &resolution.groups[0];
    assert_eq!(group.quick_group_node_ids, vec![ids[0], ids[2]]);
    assert!(group.explicit_node_ids.is_empty());
    assert_eq!(group.missing[0].node_id, ids[1]);
    assert_eq!(group.missing[0].reason, MissingReason::OutsideSelection);
    assert_eq!(pool.list_calls.load(Ordering::Relaxed), 0);
    assert_eq!(pool.requested_ids.load(Ordering::Relaxed), 6);
}

#[tokio::test]
async fn explicit_only_group_avoids_pool_listing() {
    let node = entry(1, true, false);
    let id = node.node.id;
    let pool = MockPool {
        entries: vec![node],
        ..Default::default()
    };
    let group = ProxyGroup {
        name: "explicit".into(),
        group_type: deve_sub_domain::GroupType::Select,
        members: vec![GroupMember::Node { id }],
        filter: None,
        sort_order: None,
    };
    let result = resolve_group(&group, &pool).await.expect("resolve");
    assert_eq!(result.explicit_node_ids, vec![id]);
    assert!(result.missing.is_empty());
    assert_eq!(pool.list_calls.load(Ordering::Relaxed), 0);
    assert_eq!(pool.requested_ids.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn dynamic_selection_does_not_fetch_ignored_pins() {
    let node = entry(1, true, false);
    let id = node.node.id;
    let pool = MockPool {
        entries: vec![node],
        ..Default::default()
    };
    let selector = NodeSelector {
        mode: SelectionMode::Dynamic,
        node_ids: vec![entry(2, true, false).node.id],
        ..Default::default()
    };
    let (selected, missing) = resolve_selection(&selector, &pool).await.expect("resolve");
    assert_eq!(selected, vec![id]);
    assert!(missing.is_empty());
    assert_eq!(pool.requested_ids.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn dynamic_groups_preserve_pagination_and_selector_intersection() {
    let mut entries: Vec<_> = (0..1001)
        .map(|index| {
            let mut node = entry(1, true, false);
            node.node.id = NodeId::new();
            node.node.region.value = Some(if index % 2 == 0 { "US" } else { "JP" }.into());
            node
        })
        .collect();
    entries.sort_unstable_by_key(|e| e.node.id);
    let outside = entries
        .iter()
        .find(|e| e.node.region.value.as_deref() == Some("JP"))
        .expect("outside node")
        .node
        .id;
    let expected: Vec<_> = entries
        .iter()
        .filter(|e| e.node.region.value.as_deref() == Some("US"))
        .map(|e| e.node.id)
        .collect();
    let pool = MockPool {
        entries,
        ..Default::default()
    };
    let doc = document(
        NodeSelector {
            mode: SelectionMode::Dynamic,
            filters: vec![NodeFilterRule {
                field: FilterField::Region,
                value: "US".into(),
            }],
            // Dynamic selection must not batch-fetch this ignored pin.
            node_ids: vec![NodeId::new()],
            ..Default::default()
        },
        ProxyGroup {
            name: "selected-trojan".into(),
            group_type: deve_sub_domain::GroupType::Select,
            members: vec![GroupMember::Node { id: outside }],
            filter: Some(QuickGroupFilter {
                region: None,
                protocol: Some("trojan".into()),
                tag: None,
            }),
            sort_order: None,
        },
    );
    let resolution = resolve_template(&doc, &pool).await.expect("resolve");
    assert_eq!(resolution.selected_node_ids, expected);
    assert_eq!(resolution.groups[0].quick_group_node_ids, expected);
    assert!(resolution.groups[0].explicit_node_ids.is_empty());
    assert_eq!(
        resolution.groups[0].missing[0].reason,
        MissingReason::OutsideSelection
    );
    assert_eq!(pool.list_calls.load(Ordering::Relaxed), 3);
    assert_eq!(pool.requested_ids.load(Ordering::Relaxed), 1);
}
