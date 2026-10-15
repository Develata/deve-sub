//! GEN-015: generated native memberships are not template input documents.

use super::*;

#[tokio::test]
async fn native_materialized_groups_can_exceed_template_input_limit() {
    let group_count = 4;
    let extra_nodes = 1_000;
    let spec = json!({
        "proxy-groups": (0..group_count).map(|index| json!({
            "name": format!("All-{index}"),
            "type": "select",
            "include-all-proxies": true,
        })).collect::<Vec<_>>(),
        "rules": ["MATCH,All-0"],
    })
    .to_string();
    assert!(spec.len() < 1_024, "the authored template is small");
    let db = TestDb::new(&spec, "large-native-generation").await;
    let suffix = "x".repeat(320);
    let nodes = (0..extra_nodes)
        .map(|index| {
            make_trojan(
                &NodeId::new().to_string(),
                &format!("fixture-{index:04}-{suffix}"),
                &format!("node-{index}.example.test"),
            )
        })
        .collect();
    SqliteNodePoolRepository::new_with_key(db.pool.clone(), db.master_key.clone())
        .import_nodes(nodes)
        .await
        .expect("synthetic long-name nodes");

    let request = make_request(db.template_id, "mihomo");
    let result = run(&db, request.clone(), "generate")
        .await
        .expect("materialized output may exceed the authored template limit");
    let output: deve_sub_yaml::Value =
        deve_sub_yaml::from_str(&result.content).expect("valid generated YAML");
    let groups = output["proxy-groups"].as_sequence().expect("groups");
    // WHY: exercise the intermediate native document after include-all expansion,
    // not only the final document made large by emitted proxy credentials.
    assert!(
        deve_sub_yaml::to_string(&output["proxy-groups"])
            .expect("serialized groups")
            .len()
            > deve_sub_domain::MAX_SPEC_BYTES
    );
    assert_eq!(groups.len(), group_count);
    for group in groups {
        let members = group["proxies"].as_sequence().expect("resolved members");
        assert_eq!(members.len(), extra_nodes + 3);
        assert!(members.contains(&deve_sub_yaml::Value::String(format!(
            "fixture-0999-{suffix}"
        ))));
    }
    assert_eq!(
        output["proxies"].as_sequence().expect("proxies").len(),
        extra_nodes + 3
    );
    assert_eq!(result.included_node_ids.len(), extra_nodes + 3);
    assert_eq!(
        run(&db, request, "delivery")
            .await
            .expect("published output remains deliverable")
            .content,
        result.content
    );
}
