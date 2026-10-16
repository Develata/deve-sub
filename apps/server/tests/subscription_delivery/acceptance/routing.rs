use super::*;

/// An explicitly LIMITED projection for today's routing-only contract. The
/// untouched sanitized upload is tested separately and is NOT accepted as-is.
pub(super) fn supported_routing_projection() -> String {
    UPLOADED
        .lines()
        .filter(|line| {
            !line.starts_with("mode:")
                && !line.starts_with("proxies:")
                && !line.contains("include-all-providers:")
                && !line.contains("__PROXY_PROVIDERS__")
                && !line.contains("__PROXY_NODES__")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

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

/// GEN-002/015: records the exact upload boundary instead of silently rewriting
/// the fixture or claiming that placeholders/provider expansion already works.
#[tokio::test]
async fn acceptance_uploaded_template_reports_existing_contract_blockers() {
    assert_eq!(UPLOADED.lines().count(), 350);
    let original = yaml(UPLOADED);
    assert!(original["proxies"].is_null());
    assert_eq!(original["mode"], "rule");
    assert_eq!(
        original["proxy-groups"].as_array().expect("groups").len(),
        4
    );
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let projection = supported_routing_projection();
    for (input, message) in [
        (UPLOADED.to_owned(), "unsupported Clash section"),
        (format!("mode: rule\n{projection}"), "'mode'"),
        (format!("proxies: null\n{projection}"), "'proxies'"),
        (
            format!("proxy-providers: {{}}\n{projection}"),
            "'proxy-providers'",
        ),
        (
            projection.replacen(
                "include-all-proxies: true",
                "include-all-providers: true",
                1,
            ),
            "external proxy providers",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(with_cookie(
                post_json(
                    "/api/v1/templates",
                    &json!({"name":"blocked-upload","spec_yaml":input}).to_string(),
                ),
                &cookie,
            ))
            .await
            .expect("create rejected template");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error = body_to_json(response).await;
        assert!(
            error["message"]
                .as_str()
                .expect("error message")
                .contains(message),
            "{error}"
        );
    }
    let cases = cases();
    import_cases(&app, &router, &cookie, &cases[2..3]).await;
    for placeholder in ["__PROXY_PROVIDERS__", "__PROXY_NODES__"] {
        let spec = projection.replacen(
            "      - REJECT",
            &format!("      - {placeholder}\n      - REJECT"),
            1,
        );
        let id = template(&router, &cookie, &spec).await;
        let response = generate(&router, &cookie, &id, "mihomo", "lenient").await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error = body_to_json(response).await;
        assert!(
            error["message"]
                .as_str()
                .expect("error")
                .contains(placeholder),
            "{error}"
        );
        let token = subscription(&router, &cookie, &id, "mihomo", None).await;
        assert_eq!(
            router
                .clone()
                .oneshot(get(&format!("/sub/{token}/mihomo")))
                .await
                .expect("blocked delivery")
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}

/// The supported projection retains every DNS value, policy insertion order,
/// rule and rule order. Node names must resolve and all four Unicode groups
/// remain connected. This does not test expansion of the removed placeholders.
#[tokio::test]
async fn acceptance_routing_projection_preserves_dns_rules_and_group_references() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let cases = cases();
    let ids = import_cases(&app, &router, &cookie, &cases).await;
    let spec = supported_routing_projection();
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
        assert_eq!(&members[..explicit.len()], explicit);
        let actual: BTreeSet<_> = members
            .iter()
            .map(|v| v.as_str().expect("member"))
            .collect();
        assert_eq!(actual.len(), members.len());
        assert!(actual.is_subset(&namespace), "dangling group member");
        let mut expected: BTreeSet<_> = explicit
            .iter()
            .map(|v| v.as_str().expect("member"))
            .collect();
        if before["include-all-proxies"] == true {
            expected.extend(&nodes);
        }
        assert_eq!(actual, expected, "group {}", group["name"]);
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
