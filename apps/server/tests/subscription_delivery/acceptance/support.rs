use super::*;
use deve_sub_domain::{Node, ProtocolConfig, ProtocolKind};

impl Case {
    pub(super) fn accepts(&self, profile: &str) -> bool {
        self.profiles.iter().any(|p| p == profile)
    }

    fn assert_node(&self, node: &Node, profile: &str) {
        assert!(
            !matches!(node.config, ProtocolConfig::Unsupported(_)),
            "{}: {profile}",
            self.name
        );
        let value = serde_json::to_value(node).expect("node JSON");
        for (path, expected) in &self.fields {
            // The de-facto ShadowTLS URI carries only the wrapper. Assert the
            // explicit loss below instead of claiming an inner proxy survives.
            if profile == "uri_list"
                && self.protocol == ProtocolKind::ShadowTls
                && (path.starts_with("/config/ShadowTls/inner_") || path == "/authentication")
            {
                continue;
            }
            if profile == "uri_list"
                && self.protocol == ProtocolKind::ShadowTls
                && path == "/tls/skip_cert_verify"
            {
                // Current URI parser explicitly defaults this wrapper to secure.
                assert_eq!(value.pointer(path), Some(&json!(false)));
            } else {
                assert_eq!(
                    value.pointer(path),
                    Some(expected),
                    "{} -> {profile}: {path}",
                    self.name
                );
            }
        }
        if profile == "uri_list" && self.protocol == ProtocolKind::ShadowTls {
            assert_eq!(
                value["config"]["ShadowTls"]["inner_protocol"],
                json!({"Unknown":""})
            );
            assert_eq!(value["authentication"], json!({"type":"none"}));
            assert!(
                matches!(&node.config, ProtocolConfig::ShadowTls(c) if matches!(*c.inner_config, ProtocolConfig::Unsupported(_)))
            );
        }
    }
}

pub(super) async fn import(
    router: &axum::Router,
    cookie: &str,
    kind: &str,
    input: &str,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/nodes/import",
                &json!({"source_type":kind,"content":input}).to_string(),
            ),
            cookie,
        ))
        .await
        .expect("import")
}

pub(super) async fn import_cases(
    app: &TestApp,
    router: &axum::Router,
    cookie: &str,
    cases: &[Case],
) -> Vec<String> {
    let mut ids = Vec::new();
    for case in cases {
        let response = import(router, cookie, &case.source_type, &case.input).await;
        assert_eq!(response.status(), StatusCode::OK, "{}", case.name);
        let result = body_to_json(response).await;
        assert_eq!(result["new_nodes"], 1, "{}: {result}", case.name);
        assert_eq!(result["failed"], 0);
        let outcomes = result["outcomes"].as_array().expect("outcomes");
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0]["status"], "inserted");
        let id = outcomes[0]["data"]
            .as_str()
            .expect("persisted ID")
            .to_owned();
        // Read through the real encrypted SQLite adapter, independently of
        // the import response. A successful HTTP outcome cannot hide data loss.
        let entries = app
            .state
            .pool_repo
            .get_nodes(&[deve_sub_kernel::NodeId::parse(&id).expect("node ID")])
            .await
            .expect("stored node");
        assert_eq!(entries.len(), 1);
        case.assert_node(&entries[0].node, "storage");
        let duplicate =
            body_to_json(import(router, cookie, &case.source_type, &case.input).await).await;
        assert_eq!(duplicate["new_nodes"], 0);
        assert_eq!(duplicate["duplicate_nodes"], 1);
        assert_eq!(duplicate["outcomes"][0]["data"], id);
        ids.push(id);
    }
    ids
}

pub(super) fn v3(ids: Option<&[String]>) -> String {
    json!({"apiVersion":"deve-sub.io/v1","kind":"SubscriptionTemplate","metadata":{"name":"acceptance"},"spec":{"targetProfiles":PROFILES,"nodeSelector":selection(ids),"proxyGroups":[],"rules":[]}}).to_string()
}

fn selection(ids: Option<&[String]>) -> Value {
    ids.map_or_else(
        || json!({"mode":"dynamic"}),
        |ids| json!({"mode":"fixed","nodeIds":ids}),
    )
}

pub(super) async fn template(router: &axum::Router, cookie: &str, spec: &str) -> String {
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/templates",
                &json!({"name":format!("acceptance-{}",ulid::Ulid::new()),"spec_yaml":spec})
                    .to_string(),
            ),
            cookie,
        ))
        .await
        .expect("template create");
    let status = response.status();
    let body = body_to_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["template"]["id"]
        .as_str()
        .expect("template ID")
        .to_owned()
}

pub(super) async fn generate(
    router: &axum::Router,
    cookie: &str,
    id: &str,
    profile: &str,
    mode: &str,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(with_cookie(
            post_json(
                &format!("/api/v1/templates/{id}/generate?profile={profile}&mode={mode}"),
                "",
            ),
            cookie,
        ))
        .await
        .expect("generate")
}

pub(super) async fn subscription(
    router: &axum::Router,
    cookie: &str,
    template: &str,
    profile: &str,
    ids: Option<&[String]>,
) -> String {
    let response = router.clone().oneshot(with_cookie(post_json("/api/v1/subscriptions", &json!({"name":"acceptance","slug":format!("acceptance-{}",ulid::Ulid::new()),"template_id":template,"profile":profile,"node_selection":selection(ids)}).to_string()), cookie)).await.expect("subscription create");
    let status = response.status();
    let body = body_to_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["token_plaintext"].as_str().expect("token").to_owned()
}

pub(super) fn assert_report(report: &Value, included: &[String], excluded: &[String]) {
    let actual = report["included_node_ids"].as_array().expect("included");
    assert_eq!(actual.len(), included.len());
    assert_eq!(
        actual
            .iter()
            .map(|v| v.as_str().expect("ID"))
            .collect::<BTreeSet<_>>(),
        included.iter().map(String::as_str).collect()
    );
    let actual = report["excluded"].as_array().expect("excluded");
    assert_eq!(actual.len(), excluded.len());
    assert_eq!(
        actual
            .iter()
            .map(|v| v["node_id"].as_str().expect("ID"))
            .collect::<BTreeSet<_>>(),
        excluded.iter().map(String::as_str).collect()
    );
    for entry in actual {
        assert!(
            entry["reason"]
                .as_str()
                .expect("reason")
                .contains("unsupported protocol")
        );
        assert!(
            entry["display_name"]
                .as_str()
                .expect("name")
                .starts_with("acceptance-")
        );
    }
}

pub(super) fn assert_content_type(response: &axum::response::Response, profile: &str) {
    let content_type = response.headers()["content-type"]
        .to_str()
        .expect("content type");
    let expected = match profile {
        "mihomo" => "yaml",
        "uri_list" | "shadowrocket" => "text/plain",
        _ => "application/json",
    };
    assert!(content_type.contains(expected), "{profile}: {content_type}");
}

pub(super) fn assert_output(content: &str, profile: &str, expected: &[&Case]) {
    assert!(!content.contains("__PROXY_"), "unexpanded placeholder");
    use deve_sub_protocol::container::*;
    let nodes: Vec<Node> = match profile {
        "mihomo" => parse_mihomo_yaml(content).expect("Mihomo nodes"),
        "sing-box" => parse_singbox_json(content).expect("sing-box nodes"),
        "xray" => parse_xray_json(content).expect("Xray nodes"),
        "v2ray" => parse_v2ray_json(content).expect("V2Ray nodes"),
        "shadowrocket" => {
            use ::base64::Engine;
            let bytes = ::base64::engine::general_purpose::STANDARD
                .decode(content)
                .expect("base64 share list");
            let decoded = String::from_utf8(bytes).expect("UTF-8 share list");
            decoded
                .lines()
                .map(|line| {
                    deve_sub_protocol::parse_uri(line).expect("every decoded line must be valid")
                })
                .collect()
        }
        "uri_list" => content
            .lines()
            .map(|line| {
                deve_sub_protocol::parse_uri(line).expect("every delivered line must be valid")
            })
            .collect(),
        "json" => serde_json::from_str(content).expect("canonical nodes"),
        _ => panic!("uncovered profile {profile}"),
    };
    assert_eq!(
        nodes.len(),
        expected.len(),
        "{profile}: extra, missing, or unmerged nodes"
    );
    let names: BTreeSet<_> = nodes.iter().map(|n| n.display_name.as_str()).collect();
    assert_eq!(names.len(), nodes.len(), "duplicate output node names");
    assert_eq!(names, expected.iter().map(|c| c.name.as_str()).collect());
    for case in expected {
        case.assert_node(
            nodes
                .iter()
                .find(|n| n.display_name == case.name)
                .expect("expected node"),
            profile,
        );
    }
    // An independent raw shape check catches leaked ShadowTLS wrappers and
    // wrong format roots even if a parser would otherwise discard extras.
    if ["sing-box", "xray", "v2ray"].contains(&profile) {
        let doc: Value = serde_json::from_str(content).expect("JSON document");
        let outbounds = doc["outbounds"].as_array().expect("outbounds");
        if ["xray", "v2ray"].contains(&profile) {
            for outbound in outbounds.iter().filter(|v| v["protocol"] == "vless") {
                assert_eq!(
                    outbound["settings"]["vnext"][0]["users"][0]["encryption"], "none",
                    "required client field, not inferred by the parser"
                );
            }
        }
        let wrappers = usize::from(profile == "sing-box")
            * expected
                .iter()
                .filter(|c| c.protocol == ProtocolKind::ShadowTls)
                .count();
        assert_eq!(outbounds.len(), expected.len() + wrappers);
        if wrappers > 0 {
            let wrapper = outbounds
                .iter()
                .find(|v| v["type"] == "shadowtls")
                .expect("wrapper");
            assert_eq!(wrapper["password"], "TEST_WRAPPER");
            let inner = outbounds
                .iter()
                .find(|v| v["detour"] == wrapper["tag"])
                .expect("inner detour");
            assert_eq!(inner["password"], "TEST_INNER");
        }
    }
}
