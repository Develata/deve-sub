#![allow(clippy::expect_used)]

use serde_json::{Value, json};
use std::{fs, process::Command};

fn needs() -> Value {
    let workflow: Value = serde_yaml::from_str(include_str!("../../../.github/workflows/ci.yml"))
        .expect("workflow YAML");
    workflow["jobs"]["acceptance-gate"]["needs"]
        .as_array()
        .expect("required jobs")
        .iter()
        .map(|job| {
            (
                job.as_str().expect("job name").to_owned(),
                json!({"result": "success"}),
            )
        })
        .collect()
}

fn run(needs: Value, event: &str) -> (bool, Value) {
    let directory = tempfile::tempdir().expect("temporary report directory");
    let path = directory.path().join("nested/ci-result.json");
    let output = Command::new(env!("CARGO_BIN_EXE_deve-sub-ci"))
        .args(["gate", "--output"])
        .arg(&path)
        .env("CI_NEEDS", needs.to_string())
        .env("GITHUB_EVENT_NAME", event)
        .output()
        .expect("run gate CLI");
    let report =
        serde_json::from_slice(&fs::read(path).expect("persisted report")).expect("JSON report");
    (output.status.success(), report)
}

#[test]
fn passing_cli_persists_schema_three_report() {
    let (success, report) = run(needs(), "push");
    assert!(success);
    assert_eq!(report["schema_version"], 3);
    assert_eq!(report["status"], "pass");
}

#[test]
fn cancelled_browser_lane_writes_failed_report_before_nonzero_exit() {
    let mut needs = needs();
    needs["browser-e2e"]["result"] = json!("cancelled");
    let (success, report) = run(needs, "pull_request");
    assert!(!success);
    assert_eq!(report["status"], "fail");
    assert_eq!(report["jobs"]["browser-e2e"]["status"], "cancelled");
}

#[test]
fn missing_inventory_cannot_produce_a_green_report() {
    let mut needs = needs();
    needs.as_object_mut().expect("job map").remove("inventory");
    let (success, report) = run(needs, "push");
    assert!(!success);
    assert_eq!(report["jobs"]["inventory"]["status"], "missing");
}

#[test]
fn pr_reports_the_permitted_multiarch_skip_honestly() {
    let mut needs = needs();
    needs["multiarch"]["result"] = json!("skipped");
    let (success, report) = run(needs, "pull_request");
    assert!(success);
    assert_eq!(report["jobs"]["multiarch"]["status"], "not-run");
}
