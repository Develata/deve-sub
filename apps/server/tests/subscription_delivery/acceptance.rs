//! M3/M4/M5/M6/M9: deterministic protocol-to-delivery acceptance, not connectivity.

use super::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[path = "acceptance/routing.rs"]
mod routing;
#[path = "acceptance/source.rs"]
mod source;
#[path = "acceptance/support.rs"]
mod support;
#[path = "acceptance/validators.rs"]
mod validators;
#[path = "acceptance/variants.rs"]
mod variants;
use support::*;

const PROFILES: [&str; 7] = [
    "mihomo",
    "sing-box",
    "xray",
    "v2ray",
    "shadowrocket",
    "uri_list",
    "json",
];
const UPLOADED: &str =
    include_str!("../../../../tests/fixtures/subscription-acceptance/uploaded-mihomo.yaml");

#[derive(Clone, Deserialize)]
struct Case {
    name: String,
    protocol: deve_sub_domain::ProtocolKind,
    source_type: String,
    input: String,
    profiles: Vec<String>,
    fields: BTreeMap<String, Value>,
}

fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/subscription-acceptance/protocols.json"
    ))
    .expect("hand-authored protocol fixtures")
}

/// The oracle is explicit fixture data, never the compatibility implementation
/// under test. A registry addition/removal requires an intentional matrix edit.
#[test]
fn acceptance_matrix_matches_registered_protocols() {
    let cases = cases();
    assert_eq!(cases.len(), 11);
    let kinds: std::collections::HashSet<_> = cases.iter().map(|c| c.protocol.clone()).collect();
    assert_eq!(kinds.len(), cases.len());
    for profile in PROFILES {
        let kind = deve_sub_compatibility::ProfileKind::from_kebab(profile).expect("profile");
        let expected = cases
            .iter()
            .filter(|c| c.accepts(profile))
            .map(|c| c.protocol.clone())
            .collect();
        assert_eq!(
            deve_sub_compatibility::capability_for(kind).supported_protocols,
            expected,
            "{profile}"
        );
    }
    assert_eq!(cases.iter().map(|c| c.profiles.len()).sum::<usize>(), 57);
}

/// OUT-001..007/015, NODE-001: all 77 protocol/profile cells pass through
/// import, SQLite, generation and public delivery. Unsupported cells must fail.
#[tokio::test]
async fn acceptance_individual_protocol_target_matrix() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let cases = cases();
    let ids = import_cases(&app, &router, &cookie, &cases).await;
    for (case, id) in cases.iter().zip(&ids) {
        let template = template(&router, &cookie, &v3(Some(std::slice::from_ref(id)))).await;
        for profile in PROFILES {
            let response = generate(&router, &cookie, &template, profile, "strict").await;
            let token = subscription(
                &router,
                &cookie,
                &template,
                profile,
                Some(std::slice::from_ref(id)),
            )
            .await;
            let delivered = router
                .clone()
                .oneshot(get(&format!("/sub/{token}/{profile}")))
                .await
                .expect("delivery");
            if case.accepts(profile) {
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{} -> {profile}",
                    case.name
                );
                let report = body_to_json(response).await;
                assert_report(&report, std::slice::from_ref(id), &[]);
                assert_eq!(
                    delivered.status(),
                    StatusCode::OK,
                    "{} -> {profile}",
                    case.name
                );
                assert_content_type(&delivered, profile);
                let content = body_to_string(delivered).await;
                assert_eq!(content, report["content"].as_str().expect("content"));
                assert_output(&content, profile, &[case]);
            } else {
                assert_eq!(
                    response.status(),
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "{} -> {profile}",
                    case.name
                );
                assert_eq!(body_to_json(response).await["error"], "incompatible_nodes");
                assert_eq!(
                    delivered.status(),
                    StatusCode::SERVICE_UNAVAILABLE,
                    "unsupported-only delivery must fail"
                );
                assert!(!body_to_string(delivered).await.is_empty());
            }
        }
    }
}

/// Mixed generation proves exact inclusion/exclusion accounting. Repeated
/// delivery and conditional GET exercise the persisted generation cache.
#[tokio::test]
async fn acceptance_mixed_protocol_reports_and_cached_delivery() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let cases = cases();
    let ids = import_cases(&app, &router, &cookie, &cases).await;
    let template = template(&router, &cookie, &v3(None)).await;
    for profile in PROFILES {
        let accepted: Vec<_> = cases.iter().filter(|c| c.accepts(profile)).collect();
        let included: Vec<_> = cases
            .iter()
            .zip(&ids)
            .filter(|(c, _)| c.accepts(profile))
            .map(|(_, id)| id.clone())
            .collect();
        let excluded: Vec<_> = cases
            .iter()
            .zip(&ids)
            .filter(|(c, _)| !c.accepts(profile))
            .map(|(_, id)| id.clone())
            .collect();
        let strict = generate(&router, &cookie, &template, profile, "strict").await;
        assert_eq!(
            strict.status(),
            if excluded.is_empty() {
                StatusCode::OK
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            }
        );
        let response = generate(&router, &cookie, &template, profile, "lenient").await;
        assert_eq!(response.status(), StatusCode::OK);
        let report = body_to_json(response).await;
        assert_report(&report, &included, &excluded);
        assert_output(
            report["content"].as_str().expect("content"),
            profile,
            &accepted,
        );
        let token = subscription(&router, &cookie, &template, profile, None).await;
        let path = format!("/sub/{token}/{profile}");
        let delivered = router.clone().oneshot(get(&path)).await.expect("delivery");
        assert_eq!(delivered.status(), StatusCode::OK);
        assert_content_type(&delivered, profile);
        let etag = delivered.headers()["etag"]
            .to_str()
            .expect("etag")
            .to_owned();
        let content = body_to_string(delivered).await;
        assert_output(&content, profile, &accepted);
        let cached = router.clone().oneshot(get(&path)).await.expect("cache");
        assert_eq!(cached.status(), StatusCode::OK);
        assert_eq!(body_to_string(cached).await, content);
        let unchanged = router
            .clone()
            .oneshot(get(&path).with_header("if-none-match", etag))
            .await
            .expect("conditional GET");
        assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
        assert!(body_to_string(unchanged).await.is_empty());
    }
}

/// Empty/invalid imports cannot result in an apparently successful empty
/// subscription, even when the importer uses per-entry HTTP 200 outcomes.
#[tokio::test]
async fn acceptance_empty_and_invalid_inputs_never_publish_empty_success() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    for (input, failed) in [("# empty\n\n", 0), ("invalid\ntrojan://\n", 2)] {
        let response = import(&router, &cookie, "uri_list", input).await;
        assert_eq!(response.status(), StatusCode::OK);
        let result = body_to_json(response).await;
        assert_eq!(result["new_nodes"], 0);
        assert_eq!(result["failed"], failed);
    }
    let malformed = import(&router, &cookie, "mihomo_yaml", "proxies: [[").await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    let template = template(&router, &cookie, &v3(None)).await;
    for profile in PROFILES {
        let response = generate(&router, &cookie, &template, profile, "lenient").await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body_to_json(response).await["error"], "no_compatible_nodes");
        let token = subscription(&router, &cookie, &template, profile, None).await;
        let response = router
            .clone()
            .oneshot(get(&format!("/sub/{token}/{profile}")))
            .await
            .expect("empty delivery");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
    let case = &cases()[2];
    let partial = import(
        &router,
        &cookie,
        "uri_list",
        &format!("invalid\n{}\ninvalid", case.input),
    )
    .await;
    let result = body_to_json(partial).await;
    assert_eq!(result["failed"], 2);
    assert_eq!(result["new_nodes"], 1);
    let token = subscription(&router, &cookie, &template, "mihomo", None).await;
    let delivered = router
        .clone()
        .oneshot(get(&format!("/sub/{token}/mihomo")))
        .await
        .expect("partial delivery");
    assert_eq!(delivered.status(), StatusCode::OK);
    assert_output(&body_to_string(delivered).await, "mihomo", &[case]);
}
