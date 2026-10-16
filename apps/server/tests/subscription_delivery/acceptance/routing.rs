use super::*;

fn yaml(text: &str) -> Value {
    serde_json::to_value(deve_sub_yaml::parse_generated_value(text).expect("YAML"))
        .expect("JSON view")
}

fn policy_keys(text: &str) -> Vec<String> {
    let doc = deve_sub_yaml::parse_generated_value(text).expect("YAML");
    doc["dns"]["nameserver-policy"]
        .as_mapping()
        .expect("ordered policy")
        .keys()
        .map(|key| key.as_str().expect("policy key").to_owned())
        .collect()
}

/// Only empty pool insertion slots are normalized. Actual nodes/providers and
/// non-rule modes remain explicit input errors.
#[tokio::test]
async fn acceptance_uploaded_template_rejects_nonempty_nodes_and_external_providers() {
    assert_eq!(yaml(UPLOADED)["rules"].as_array().expect("rules").len(), 23);
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    for (input, message) in [
        (
            UPLOADED.replace("mode: rule", "mode: direct"),
            "mode must be rule",
        ),
        (
            UPLOADED.replace("proxies: null", "proxies: [{name: embedded}]"),
            "proxies must be null or an empty list",
        ),
        (
            format!("proxy-providers: {{}}\n{UPLOADED}"),
            "'proxy-providers'",
        ),
        (
            UPLOADED.replacen("include-all-providers: true", "use: [external]", 1),
            "external proxy providers",
        ),
        (
            UPLOADED.replacen("name: 🚀 节点选择", "name: __PROXY_NODES__", 1),
            "reserved or invalid group name",
        ),
        (
            UPLOADED.replacen(
                "type: select",
                "type: select\n    default-selected: __PROXY_NODES__",
                1,
            ),
            "only supported in proxies lists",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(with_cookie(
                post_json(
                    "/api/v1/templates",
                    &json!({"name":"invalid-template", "spec_yaml":input}).to_string(),
                ),
                &cookie,
            ))
            .await
            .expect("create");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error = body_to_json(response).await;
        assert!(
            error["message"].as_str().expect("error").contains(message),
            "{error}"
        );
    }
    // All accepted empty insertion forms retain the same routing semantics.
    for input in [
        UPLOADED.to_owned(),
        UPLOADED.replace("proxies: null", "proxies: []"),
        UPLOADED.replace("proxies: null\n", ""),
    ] {
        template(&router, &cookie, &input).await;
    }
}

/// The uploaded template retains every DNS value, ordered policy and representative
/// rule. Expansion preserves token position and all Unicode group references.
#[tokio::test]
async fn acceptance_uploaded_template_expands_nodes_and_preserves_dns_rules() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let cases = cases();
    let ids = import_cases(&app, &router, &cookie, &cases).await;
    let spec = UPLOADED.to_owned();
    let id = template(&router, &cookie, &spec).await;
    let response = generate(&router, &cookie, &id, "mihomo", "lenient").await;
    assert_eq!(response.status(), StatusCode::OK);
    let report = body_to_json(response).await;
    let included = ids
        .iter()
        .zip(&cases)
        .filter(|(_, c)| c.accepts("mihomo"))
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    assert_report(&report, &included, &ids[6..7]);
    let notices = report["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .filter(|w| {
            w.as_str()
                .is_some_and(|s| s.contains("provider placeholder"))
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(notices.len(), 3);
    let cached = body_to_json(generate(&router, &cookie, &id, "mihomo", "lenient").await).await;
    for notice in &notices {
        assert!(
            cached["warnings"]
                .as_array()
                .expect("cached notices")
                .contains(notice)
        );
    }
    let token = subscription(&router, &cookie, &id, "mihomo", None).await;
    let response = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/mihomo")))
        .await
        .expect("delivery");
    assert_eq!(response.status(), StatusCode::OK);
    let content = body_to_string(response).await;
    assert_output(
        &content,
        "mihomo",
        &cases
            .iter()
            .filter(|c| c.accepts("mihomo"))
            .collect::<Vec<_>>(),
    );
    assert_routing(&content, &spec);
}

pub(super) fn assert_routing(content: &str, spec: &str) {
    let output = yaml(content);
    let original = yaml(UPLOADED);
    assert_eq!(output["mode"], "rule");
    assert_eq!(output["dns"], original["dns"]);
    assert_eq!(output["rules"], original["rules"]);
    assert_eq!(policy_keys(content), policy_keys(UPLOADED));
    assert_eq!(
        output["rules"].as_array().expect("rules").last(),
        Some(&json!("MATCH,🚀 节点选择"))
    );
    let groups = output["proxy-groups"].as_array().expect("groups");
    let nodes: BTreeSet<_> = output["proxies"]
        .as_array()
        .expect("nodes")
        .iter()
        .map(|n| n["name"].as_str().expect("name"))
        .collect();
    let mut namespace = nodes.clone();
    namespace.extend(
        groups
            .iter()
            .map(|g| g["name"].as_str().expect("group name")),
    );
    namespace.extend(["REJECT", "DIRECT"]);
    let authored = yaml(spec);
    assert_eq!(groups.len(), 4);
    for (group, before) in groups.iter().zip(
        authored["proxy-groups"]
            .as_array()
            .expect("authored groups"),
    ) {
        assert_eq!(group["name"], before["name"]);
        assert_eq!(group["type"], before["type"]);
        let members = group["proxies"].as_array().expect("members");
        let explicit = before["proxies"].as_array().expect("authored members");
        let mut expected = Vec::new();
        for member in explicit.iter().map(|v| v.as_str().expect("member")) {
            match member {
                "__PROXY_PROVIDERS__" => {}
                "__PROXY_NODES__" => expected.extend(nodes.iter().copied()),
                other => expected.push(other),
            }
        }
        if before["include-all-proxies"] == true {
            expected.extend(nodes.iter().copied());
        }
        let mut seen = BTreeSet::new();
        expected.retain(|member| seen.insert(*member));
        let actual = members
            .iter()
            .map(|v| v.as_str().expect("member"))
            .collect::<Vec<_>>();
        assert_eq!(
            actual, expected,
            "group {} token position/order",
            group["name"]
        );
        assert!(
            actual.iter().all(|m| namespace.contains(m)),
            "dangling group member"
        );
        for flag in [
            "include-all",
            "include-all-proxies",
            "include-all-providers",
        ] {
            assert!(group.get(flag).is_none());
        }
    }
    for resolver in output["dns"]["nameserver"].as_array().expect("resolvers") {
        let (_, group) = resolver
            .as_str()
            .expect("DNS string")
            .split_once('#')
            .expect("group suffix");
        assert!(namespace.contains(group));
    }
}
