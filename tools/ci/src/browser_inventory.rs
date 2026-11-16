//! Keep actual browser execution local and its complete catalogue registered.
use super::*;

pub(super) fn validate(workflow: &Value) -> Result<()> {
    let jobs = &workflow["jobs"];
    for job in jobs.as_object().context("jobs")?.values() {
        for step in array(&job["steps"])? {
            let run = step["run"].as_str().unwrap_or("");
            ensure!(
                ![
                    "playwright install",
                    "cli.js install",
                    "verify-locked-browser.cjs",
                    "--project functional-desktop",
                    "--project functional-mobile"
                ]
                .iter()
                .any(|s| run.contains(s)),
                "CI cannot provision, preflight or execute browsers"
            );
        }
    }
    let http = &jobs["http-api-lifecycle"];
    ensure!(
        named_step(http, "Run HTTP API and process lifecycle without a browser")?
            == &json!({
        "name":"Run HTTP API and process lifecycle without a browser",
        "env":{"PLAYWRIGHT_BROWSERS_PATH":"${{ runner.temp }}/deve-sub-no-browser"},
        "run":"python3 scripts/ci/suite.py --profile ci-non-browser --output \"$RUNNER_TEMP/http-api-lifecycle\""}),
        "HTTP and lifecycle coverage cannot be filtered or suppressed"
    );
    ensure!(
        named_step(http, "Install test runner without browsers")?
            == &json!({
        "name":"Install test runner without browsers", "env":{"PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD":"1"},
        "run":"npm ci", "working-directory":"tests/e2e"}),
        "runner dependencies must not acquire browsers"
    );
    ensure!(
        named_step(
            &jobs["installer-lifecycle"],
            "Verify native installer lifecycle"
        )? == &json!({
        "name":"Verify native installer lifecycle", "run":"sudo apt-get update\nsudo apt-get install -y bubblewrap\nsudo python3 scripts/tests/test_install.py --required --binary target/release/deve-sub --web-dir apps/web/dist\n"}),
        "required Linux installer cannot skip prerequisites or failures"
    );
    for name in ["http-api-lifecycle", "installer-lifecycle"] {
        ensure!(
            jobs[name]["needs"] == json!(["rust-gate", "web-wasm", "build-release"]),
            "runtime consumers require exact producers"
        );
        ensure!(
            named_step(&jobs[name], "Make binary executable")?["run"]
                .as_str()
                .is_some_and(|r| r.contains("artifact.py verify --kind binary")
                    && r.contains("artifact.py verify --kind wasm")),
            "both producer manifests remain verified"
        );
    }
    let local: Value =
        serde_json::from_str(include_str!("../../../tests/acceptance/local-browser.json"))?;
    let api: Value =
        serde_json::from_str(include_str!("../../../tests/acceptance/non-browser.json"))?;
    let normal: Value =
        serde_json::from_str(include_str!("../../../tests/acceptance/normal-ci.json"))?;
    ensure!(
        local["profile"] == "local-browser" && api["profile"] == "ci-non-browser",
        "separate evidence profiles required"
    );
    for (group, count) in [
        ("legacy", 16),
        ("functional-desktop", 48),
        ("functional-mobile", 48),
    ] {
        ensure!(
            array(&local["groups"][group]["cases"])?.len() == count,
            "local browser cases lost"
        );
    }
    ensure!(
        array(&api["groups"]["http-api"]["cases"])?.len() == 15
            && array(&api["groups"]["lifecycle"]["cases"])?.len() == 3,
        "non-browser coverage lost"
    );
    ensure!(
        local["groups"]["normal"]["catalogue"] == "normal-ci.json: all kind=playwright cases",
        "normal browser catalogue must retain all cases"
    );
    let normal_cases = normal["lanes"]
        .as_object()
        .context("normal lanes")?
        .values()
        .filter(|v| v["kind"] == "playwright")
        .map(|v| v["cases"].as_array().map_or(0, Vec::len))
        .sum::<usize>();
    ensure!(normal_cases == 39, "full normal browser scope changed");
    Ok(())
}
