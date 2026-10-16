use super::*;
use std::process::{Command, Output};

fn assert_version(profile: &str) {
    let expected = match profile {
        "mihomo" => "Mihomo Meta v1.19.30",
        "sing-box" => "sing-box version 1.14.3",
        "xray" => "Xray 26.3.27",
        _ => panic!("unvalidated client"),
    };
    let output = Command::new("timeout")
        .args([
            "--kill-after=2s",
            "5s",
            profile,
            if profile == "mihomo" { "-v" } else { "version" },
        ])
        .output()
        .expect("installed pinned core");
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).starts_with(expected),
        "install the versions pinned in scripts/install-validators.py"
    );
}

fn check(profile: &str, content: &str) -> Output {
    let dir = tempfile::tempdir().expect("isolated core directory");
    let path = dir.path().join(if profile == "mihomo" {
        "config.yaml"
    } else {
        "config.json"
    });
    std::fs::write(&path, content).expect("actual delivered bytes");
    // Self-authored GeoSiteList: PRIVATE and CN -> Domain(home.arpa).
    // Mihomo verifies CN even when the authored policy only references PRIVATE. This tiny local
    // protobuf satisfies the upload's geosite:private DNS policy without any
    // remote geodata. Schema: MetaCubeX/mihomo v1.19.30,
    // component/geodata/router/config.proto (linked in the acceptance guide).
    std::fs::write(
        dir.path().join("GeoSite.dat"),
        b"\x0a\x18\x0a\x07PRIVATE\x12\x0d\x08\x02\x12\x09home.arpa\x0a\x13\x0a\x02CN\x12\x0d\x08\x02\x12\x09home.arpa",
    )
    .expect("synthetic geosite");
    let mut command = Command::new("timeout");
    command.args(["--kill-after=2s", "10s", profile]);
    match profile {
        "mihomo" => {
            command.args(["-t", "-d"]).arg(dir.path()).arg("-f");
        }
        "sing-box" => {
            command.args(["check", "-c"]);
        }
        "xray" => {
            command.args(["run", "-test", "-config"]);
        }
        _ => unreachable!(),
    }
    let output = command
        .arg(path)
        .current_dir(dir.path())
        .output()
        .expect("run installed core");
    assert!(
        if profile == "xray" {
            matches!(output.status.code(), Some(0 | 23))
        } else {
            matches!(output.status.code(), Some(0 | 1))
        },
        "core failed to validate/exit normally: {:?}",
        output.status
    );
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !log.contains("download"),
        "validation must not fetch remote resources: {log}"
    );
    output
}

/// 31 delivered configurations accepted by pinned cores, plus semantic-negative
/// controls. This is offline configuration validation, never proxy connectivity.
#[tokio::test]
#[ignore = "requires checksum-pinned mihomo, sing-box and xray on PATH"]
async fn acceptance_validators_accept_31_delivered_protocol_configurations() {
    validate_matrix().await;
    let mut variants = 0;
    for case in current_formats::variants() {
        for profile in &case.profiles {
            let content = current_formats::delivered(&case, profile).await;
            let result = check(profile, &content);
            assert!(
                result.status.success(),
                "{profile}: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            variants += 1;
        }
    }
    assert_eq!(variants, 6);
    for profile in ["mihomo", "sing-box", "xray"] {
        let invalid = match profile {
            "mihomo" => {
                "proxies: [{name: invalid, type: invalid-protocol, server: unused.example, port: 443}]"
            }
            "sing-box" => r#"{"outbounds":[{"type":"invalid-protocol","tag":"invalid"}]}"#,
            _ => r#"{"outbounds":[{"protocol":"invalid-protocol","tag":"invalid","settings":{}}]}"#,
        };
        assert!(
            !check(profile, invalid).status.success(),
            "no-op core accepted invalid semantics"
        );
    }
}

async fn validate_matrix() {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let cases = cases();
    let ids = import_cases(&app, &router, &cookie, &cases).await;
    let template = template(&router, &cookie, &v3(None)).await;
    let mut count = 0;
    for profile in ["mihomo", "sing-box", "xray"] {
        assert_version(profile);
        for (case, id) in cases.iter().zip(&ids).filter(|(c, _)| c.accepts(profile)) {
            let token = subscription(
                &router,
                &cookie,
                &template,
                profile,
                Some(std::slice::from_ref(id)),
            )
            .await;
            let response = router
                .clone()
                .oneshot(get(&format!("/sub/{token}/{profile}")))
                .await
                .expect("delivered config");
            assert_eq!(response.status(), StatusCode::OK);
            let content = body_to_string(response).await;
            assert_output(&content, profile, &[case]);
            let output = check(profile, &content);
            let log = format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.status.success(), "{} -> {profile}: {log}", case.name);
            count += 1;
        }
    }
    for profile in ["mihomo", "sing-box", "xray"] {
        let token = subscription(&router, &cookie, &template, profile, None).await;
        let response = router
            .clone()
            .oneshot(get(&format!("/sub/{token}/{profile}")))
            .await
            .expect("mixed delivery");
        assert_eq!(response.status(), StatusCode::OK);
        let content = body_to_string(response).await;
        assert_output(
            &content,
            profile,
            &cases
                .iter()
                .filter(|c| c.accepts(profile))
                .collect::<Vec<_>>(),
        );
        let result = check(profile, &content);
        assert!(
            result.status.success(),
            "mixed {profile}: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(count, 25);
}

/// The actual delivered uploaded routing template, including DNS geosite,
/// Unicode group suffixes and ordered policies, validates with local geodata.
#[tokio::test]
#[ignore = "requires checksum-pinned mihomo; geosite data is synthesized locally"]
async fn acceptance_validators_validate_routing_dns_with_local_geosite() {
    assert_version("mihomo");
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let cases = cases();
    import_cases(&app, &router, &cookie, &cases).await;
    let spec = UPLOADED.to_owned();
    let id = template(&router, &cookie, &spec).await;
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
    routing::assert_routing(&content, &spec);
    let result = check("mihomo", &content);
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
