use super::*;

/// PARSE-024/025/027, GEN-014: protocol support is insufficient when the
/// selected transport, version or obfuscation is incompatible with the target.
#[tokio::test]
async fn acceptance_transport_version_and_obfuscation_exclusions_are_reported() {
    let fixtures = cases();
    let mut xhttp = fixtures[0].clone();
    xhttp.input = xhttp.input.replace(
        "type=tcp",
        "type=xhttp&path=/split&host=front.example&mode=packet-up",
    );
    xhttp
        .fields
        .insert("/transport/kind".into(), json!("xhttp"));
    xhttp
        .fields
        .insert("/transport/path".into(), json!("/split"));
    xhttp
        .fields
        .insert("/transport/host".into(), json!("front.example"));
    xhttp
        .fields
        .insert("/transport/xhttp_mode".into(), json!("packet-up"));
    xhttp.profiles = ["mihomo", "xray", "uri_list", "json"]
        .map(str::to_owned)
        .to_vec();
    let mut snell3 = fixtures[9].clone();
    snell3.input = snell3.input.replace("version=4", "version=3");
    snell3
        .fields
        .insert("/config/Snell/version".into(), json!("V3"));
    snell3.profiles.retain(|p| p != "sing-box");
    let mut restls = fixtures[9].clone();
    restls.source_type = "mihomo_yaml".into();
    restls.input = "proxies: [{name: acceptance-snell, type: snell, server: snell.example, port: 443, psk: TEST_SNELL, version: 4, obfs-opts: {mode: restls, host: cover.example}}]".into();
    restls
        .fields
        .insert("/config/Snell/obfs/mode".into(), json!("Restls"));
    restls.profiles.retain(|p| p != "sing-box");
    for (case, targets, reason) in [
        (
            xhttp,
            vec!["sing-box", "v2ray", "shadowrocket"],
            "unsupported transport 'xhttp'",
        ),
        (snell3, vec!["sing-box"], "unsupported protocol version '3'"),
        (restls, vec!["sing-box"], "unsupported obfs mode 'restls'"),
    ] {
        let app = TestApp::new().await;
        let router = app.router();
        let cookie = setup_and_login(&router).await;
        let batch = [fixtures[2].clone(), case];
        let ids = import_cases(&app, &router, &cookie, &batch).await;
        let id = template(&router, &cookie, &v3(None)).await;
        for profile in targets {
            let strict = generate(&router, &cookie, &id, profile, "strict").await;
            assert_eq!(strict.status(), StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(body_to_json(strict).await["error"], "incompatible_nodes");
            let response = generate(&router, &cookie, &id, profile, "lenient").await;
            assert_eq!(response.status(), StatusCode::OK);
            let report = body_to_json(response).await;
            assert_eq!(report["included_node_ids"], json!([ids[0]]));
            let excluded = report["excluded"].as_array().expect("excluded");
            assert_eq!(excluded.len(), 1);
            assert_eq!(excluded[0]["node_id"], ids[1]);
            assert!(
                excluded[0]["reason"]
                    .as_str()
                    .expect("reason")
                    .contains(reason)
            );
            let token = subscription(&router, &cookie, &id, profile, None).await;
            let response = router
                .clone()
                .oneshot(get(&format!("/sub/{token}/{profile}")))
                .await
                .expect("delivery");
            assert_eq!(response.status(), StatusCode::OK);
            assert_output(&body_to_string(response).await, profile, &[&batch[0]]);
        }
    }
}

/// Known enum names are not support claims. Unknown/non-typed configs remain
/// inspectable in storage and are excluded from every generated profile.
#[tokio::test]
async fn acceptance_unimplemented_protocols_are_not_treated_as_supported() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let fixtures = cases();
    let supported_ids = import_cases(&app, &router, &cookie, &fixtures[2..3]).await;
    let mut unsupported_ids = BTreeSet::new();
    for kind in ["socks5", "http", "hysteria", "ssh", "future-protocol"] {
        let input = format!(
            "proxies:\n  - {{name: unsupported-{kind}, type: {kind}, server: unused.example, port: 443}}\n"
        );
        let response = import(&router, &cookie, "mihomo_yaml", &input).await;
        assert_eq!(response.status(), StatusCode::OK);
        let result = body_to_json(response).await;
        assert_eq!(result["new_nodes"], 1);
        let id = result["outcomes"][0]["data"]
            .as_str()
            .expect("stored unsupported ID")
            .to_owned();
        let nodes = app
            .state
            .pool_repo
            .get_nodes(&[deve_sub_kernel::NodeId::parse(&id).expect("ID")])
            .await
            .expect("stored unsupported");
        assert!(
            matches!(&nodes[0].node.config, deve_sub_domain::ProtocolConfig::Unsupported(config) if config.raw["type"] == kind)
        );
        unsupported_ids.insert(id);
    }
    assert_eq!(unsupported_ids.len(), 5);
    let id = template(&router, &cookie, &v3(None)).await;
    for profile in PROFILES {
        let response = generate(&router, &cookie, &id, profile, "lenient").await;
        assert_eq!(response.status(), StatusCode::OK);
        let report = body_to_json(response).await;
        assert_eq!(report["included_node_ids"], json!(supported_ids));
        let excluded = report["excluded"].as_array().expect("excluded");
        assert_eq!(excluded.len(), 5);
        assert_eq!(
            excluded
                .iter()
                .map(|v| v["node_id"].as_str().expect("ID").to_owned())
                .collect::<BTreeSet<_>>(),
            unsupported_ids
        );
        for node in excluded {
            assert!(!node["reason"].as_str().expect("reason").is_empty());
        }
        let token = subscription(&router, &cookie, &id, profile, None).await;
        let response = router
            .clone()
            .oneshot(get(&format!("/sub/{token}/{profile}")))
            .await
            .expect("delivery");
        assert_eq!(response.status(), StatusCode::OK);
        assert_output(&body_to_string(response).await, profile, &[&fixtures[2]]);
    }
}
