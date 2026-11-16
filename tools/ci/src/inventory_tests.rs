#![allow(clippy::expect_used)]

use super::*;

fn workflow() -> Value {
    deve_sub_yaml::parse_source_json(include_str!("../../../.github/workflows/ci.yml"))
        .expect("workflow YAML")
}

fn packages(workflow: &Value) -> BTreeSet<String> {
    workflow["jobs"]["test"]["strategy"]["matrix"]["include"]
        .as_array()
        .expect("matrix")
        .iter()
        .flat_map(|row| {
            row["crates"]
                .as_str()
                .expect("packages")
                .split_whitespace()
                .skip(1)
                .step_by(2)
                .map(str::to_owned)
        })
        .collect()
}

fn rejected(mutate: impl FnOnce(&mut Value)) {
    let mut value = workflow();
    let members = packages(&value);
    mutate(&mut value);
    assert!(validate(&value, &members).is_err(), "mutation was accepted");
}

#[test]
fn current_complete_workflow_passes() {
    let workflow = workflow();
    assert_eq!(
        validate(&workflow, &packages(&workflow)).expect("valid policy"),
        9
    );
}

#[test]
fn normal_case_lanes_gate_and_validators_cannot_be_weakened() {
    rejected(|workflow| {
        workflow["jobs"]["normal-non-browser"]["strategy"]["matrix"]["include"]
            .as_array_mut()
            .expect("matrix")
            .pop();
    });
    rejected(|workflow| {
        workflow["jobs"]["normal-non-browser"]["strategy"]["matrix"]["include"][0] =
            json!({"lane":"browser"});
    });
    rejected(|workflow| {
        workflow["jobs"]["normal-non-browser"]["strategy"]["max-parallel"] = json!(8)
    });
    rejected(|workflow| {
        workflow["jobs"]["normal-non-browser"]["strategy"]["fail-fast"] = json!(true)
    });
    rejected(|workflow| workflow["jobs"]["normal-non-browser-gate"]["if"] = json!("success()"));
    for (job, name) in [
        ("normal-non-browser", "Run complete normal lane"),
        ("normal-non-browser", "Verify normal candidate artifacts"),
        (
            "normal-non-browser-gate",
            "Require complete non-browser normal scope",
        ),
    ] {
        for key in ["if", "continue-on-error", "run"] {
            rejected(|workflow| {
                let step = workflow["jobs"][job]["steps"]
                    .as_array_mut()
                    .expect("steps")
                    .iter_mut()
                    .find(|step| step["name"] == name)
                    .expect("normal step");
                step[key] = json!("false");
            });
        }
    }
}

#[test]
fn metadata_includes_only_workspace_members_and_rejects_missing_members() {
    let mut metadata = json!({
        "workspace_members": ["local"],
        "packages": [{"id": "local", "name": "deve-sub-ci"}, {"id": "external", "name": "serde"}],
    });
    assert_eq!(
        workspace(&metadata).expect("workspace"),
        BTreeSet::from(["deve-sub-ci".to_owned()])
    );
    metadata["workspace_members"] = json!(["missing"]);
    assert!(workspace(&metadata).is_err());
    metadata["workspace_members"] = json!([]);
    assert!(workspace(&metadata).is_err());
}

#[test]
fn missing_duplicate_or_injected_package_blocks() {
    for crates in [
        "-p deve-sub-ci",
        "-p deve-sub-ci -p deve-sub-ci",
        "-p deve-sub-ci; echo bad",
        "-p",
    ] {
        rejected(|workflow| {
            workflow["jobs"]["test"]["strategy"]["matrix"]["include"][0]["crates"] = json!(crates)
        });
    }
    rejected(|workflow| {
        workflow["jobs"]["test"]["strategy"]["matrix"]["include"]
            .as_array_mut()
            .expect("matrix")
            .pop();
    });
}

#[test]
fn replaced_filtered_skipped_or_suppressed_rust_command_blocks() {
    for (key, value) in [
        ("run", json!("true")),
        (
            "run",
            json!("cargo test --locked --all-targets --all-features $CI_PACKAGES one_test"),
        ),
        ("if", json!("false")),
        ("continue-on-error", json!(true)),
        (
            "run",
            json!("cargo test --locked --all-targets --all-features $CI_PACKAGES || true"),
        ),
    ] {
        rejected(|workflow| {
            let steps = workflow["jobs"]["test"]["steps"]
                .as_array_mut()
                .expect("steps");
            let command = steps
                .iter_mut()
                .find(|step| step.get("run").is_some())
                .expect("command");
            command[key] = value;
        });
    }
}

#[test]
fn every_missing_final_gate_leaf_blocks() {
    for leaf in REQUIRED_JOBS {
        rejected(|workflow| {
            workflow["jobs"]["ci-non-browser-gate"]["needs"]
                .as_array_mut()
                .expect("needs")
                .retain(|job| job != leaf)
        });
    }
    rejected(|workflow| workflow["jobs"]["ci-non-browser-gate"]["if"] = json!("success()"));
}

#[test]
fn baseline_jobs_cannot_ignore_failures_or_skip() {
    for leaf in REQUIRED_JOBS {
        rejected(|workflow| workflow["jobs"][*leaf]["continue-on-error"] = json!(true));
        rejected(|workflow| workflow["jobs"][*leaf]["if"] = json!("false"));
    }
}

#[test]
fn every_job_has_a_finite_deadline() {
    for leaf in REQUIRED_JOBS.iter().copied().chain(["ci-non-browser-gate"]) {
        for timeout in [
            Value::Null,
            json!(0),
            json!(91),
            json!("${{ matrix.timeout }}"),
        ] {
            rejected(|workflow| workflow["jobs"][leaf]["timeout-minutes"] = timeout);
        }
    }
}

#[test]
fn main_and_release_runs_cannot_cancel_each_other() {
    rejected(|workflow| workflow["concurrency"]["group"] = json!("ci-${{ github.ref }}"));
    rejected(|workflow| workflow["concurrency"]["cancel-in-progress"] = json!(true));
}

#[test]
fn non_browser_commands_and_installation_policy_cannot_be_weakened() {
    for (job, name) in [
        (
            "http-api-lifecycle",
            "Run HTTP API and process lifecycle without a browser",
        ),
        ("installer-lifecycle", "Verify native installer lifecycle"),
    ] {
        for key in ["run", "if", "continue-on-error"] {
            rejected(|w| {
                let step = w["jobs"][job]["steps"]
                    .as_array_mut()
                    .expect("steps")
                    .iter_mut()
                    .find(|s| s["name"] == name)
                    .expect("step");
                step[key] = json!("false");
            });
        }
    }
    rejected(|w| {
        w["jobs"]["http-api-lifecycle"]["steps"]
            .as_array_mut()
            .expect("steps")
            .push(json!({"run":"npx playwright install chromium"}));
    });
    rejected(|w| {
        w["jobs"]["http-api-lifecycle"]["steps"]
            .as_array_mut()
            .expect("steps")
            .push(json!({"run":"node verify-locked-browser.cjs"}));
    });
    for job in ["test", "normal-non-browser"] {
        rejected(|w| w["jobs"][job]["strategy"]["fail-fast"] = json!(true));
        rejected(|w| w["jobs"][job]["strategy"]["matrix"]["exclude"] = json!([]));
    }
}

#[path = "docker_inventory_tests.rs"]
mod docker_tests;
