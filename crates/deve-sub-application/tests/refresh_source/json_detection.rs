use super::*;
use deve_sub_domain::{NodeFilter, NodePoolRepository, ProtocolConfig};

/// SRC-002/PARSE-007: an Auto source must publish the same usable Trojan and
/// VLESS nodes as explicit Xray parsing, and deduplicate them on refresh.
#[tokio::test]
async fn auto_xray_refresh_publishes_typed_nodes_and_preserves_identity() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );
    let source = create_source_typed(&source_repo, "auto-xray", SourceType::Auto).await;
    let body = include_str!("../fixtures/xray-auto.json");
    let expected =
        source::parse_content(SourceType::XrayJson, None, body.as_bytes()).expect("explicit Xray");
    let mut first_ids = Vec::new();

    for version in 1..=2 {
        let fetcher = MockFetcher::new(vec![MockResponse::Ok {
            body: body.as_bytes().to_vec(),
            etag: None,
            content_type: Some("application/json".into()),
        }]);
        let result = run_refresh(
            &source_repo,
            &snapshot_repo,
            &pool_repo,
            &db.pool,
            &fetcher,
            &StubGeoIp,
            source.id,
        )
        .await
        .expect("refresh");
        assert_eq!(result.snapshot.version, version);
        assert_eq!(result.snapshot.node_count, 2);
        assert_eq!(result.reconcile.missing_nodes, 0);
        if version == 1 {
            assert_eq!(result.reconcile.new_nodes, 2);
        } else {
            assert_eq!(result.reconcile.new_nodes, 0);
            assert_eq!(result.reconcile.duplicate_nodes, 2);
        }

        let nodes = pool_repo
            .list_nodes(&NodeFilter::default(), None, 10)
            .await
            .expect("nodes");
        assert_eq!(nodes.len(), 2);
        for expected in &expected {
            let expected = expected.node.as_ref().expect("explicit node");
            let actual = nodes
                .iter()
                .find(|entry| entry.node.display_name == expected.display_name)
                .expect("same node name");
            assert_eq!(actual.node.protocol, expected.protocol);
            assert_eq!(actual.node.config, expected.config);
            assert_eq!(actual.node.endpoint, expected.endpoint);
            assert_eq!(actual.node.authentication, expected.authentication);
            assert_eq!(actual.node.tls, expected.tls);
            assert_eq!(actual.node.transport, expected.transport);
            assert!(!actual.missing_from_source);
        }
        let ids: Vec<_> = nodes.iter().map(|entry| entry.node.id).collect();
        if first_ids.is_empty() {
            first_ids = ids;
        } else {
            assert_eq!(ids, first_ids);
        }
    }
}

/// SRC-002/006: unsupported nodes are intentionally preserved data, so a
/// later refresh containing them must not be mistaken for zero parsed nodes.
#[tokio::test]
async fn auto_singbox_unknown_refresh_preserves_raw_data_with_existing_snapshot() {
    let db = TestDb::new().await;
    let source_repo = SqliteSourceRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );
    let snapshot_repo = SqliteSourceSnapshotRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new_with_key(
        db.pool.clone(),
        std::sync::Arc::clone(&db.master_key),
    );
    let source = create_source_typed(&source_repo, "auto-unknown", SourceType::Auto).await;
    let outbound = serde_json::json!({
        "type": "future-protocol", "tag": "unknown", "server": "example.com",
        "server_port": 443, "fixture_secret": "TEST_PASSWORD"
    });
    let body = serde_json::json!({ "outbounds": [outbound] }).to_string();

    for version in 1..=2 {
        let fetcher = MockFetcher::new(vec![MockResponse::Ok {
            body: body.as_bytes().to_vec(),
            etag: None,
            content_type: Some("application/json".into()),
        }]);
        let result = run_refresh(
            &source_repo,
            &snapshot_repo,
            &pool_repo,
            &db.pool,
            &fetcher,
            &StubGeoIp,
            source.id,
        )
        .await
        .expect("preserved unsupported refresh");
        assert_eq!(result.snapshot.version, version);
        assert_eq!(result.snapshot.node_count, 1);
    }

    let nodes = pool_repo
        .list_nodes(&NodeFilter::default(), None, 10)
        .await
        .expect("preserved nodes");
    assert_eq!(nodes.len(), 1);
    let ProtocolConfig::Unsupported(node) = &nodes[0].node.config else {
        panic!("unknown node must remain unsupported");
    };
    assert_eq!(node.raw, outbound);
    assert_eq!(node.raw_format.as_deref(), Some("singbox-json"));
    let statuses: Vec<(String,)> =
        sqlx::query_as("SELECT parse_status FROM source_items ORDER BY rowid")
            .fetch_all(&db.pool)
            .await
            .expect("source item statuses");
    assert_eq!(statuses, vec![("unsupported".into(),); 2]);
}
