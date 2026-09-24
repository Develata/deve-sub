#![allow(clippy::expect_used)]

use super::*;

fn workflow() -> Value {
    serde_yaml::from_str(include_str!("../../../.github/workflows/ci.yml")).expect("workflow YAML")
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
            workflow["jobs"]["acceptance-gate"]["needs"]
                .as_array_mut()
                .expect("needs")
                .retain(|job| job != leaf)
        });
    }
    rejected(|workflow| workflow["jobs"]["acceptance-gate"]["if"] = json!("success()"));
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
    for leaf in REQUIRED_JOBS.iter().copied().chain(["acceptance-gate"]) {
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
fn browser_coverage_cannot_omit_duplicate_or_filter_a_suite() {
    for index in 0..4 {
        rejected(|workflow| {
            workflow["jobs"]["browser-e2e"]["strategy"]["matrix"]["include"]
                .as_array_mut()
                .expect("matrix")
                .remove(index);
        });
        rejected(|workflow| {
            workflow["jobs"]["browser-e2e"]["strategy"]["matrix"]["include"][index]["args"] =
                json!("--grep one_test")
        });
    }
    rejected(|workflow| {
        let rows = workflow["jobs"]["browser-e2e"]["strategy"]["matrix"]["include"]
            .as_array_mut()
            .expect("matrix");
        rows.push(rows[0].clone());
    });
}

#[test]
fn browser_lifecycle_and_lane_commands_cannot_be_suppressed() {
    for name in ["Run browser lane", "Verify browser process lifecycle"] {
        for (key, value) in [
            ("continue-on-error", json!(true)),
            ("if", json!("false")),
            ("run", json!("true")),
        ] {
            rejected(|workflow| {
                let steps = workflow["jobs"]["browser-e2e"]["steps"]
                    .as_array_mut()
                    .expect("steps");
                let step = steps
                    .iter_mut()
                    .find(|step| step["name"] == name)
                    .expect("browser step");
                step[key] = value;
            });
        }
    }
}

#[test]
fn matrices_cannot_hide_failures_or_exclude_lanes() {
    for job in ["test", "browser-e2e"] {
        rejected(|workflow| workflow["jobs"][job]["strategy"]["fail-fast"] = json!(true));
        rejected(|workflow| workflow["jobs"][job]["strategy"]["matrix"]["exclude"] = json!([]));
        rejected(|workflow| workflow["jobs"][job]["env"] = json!({"RUSTFLAGS": "changed"}));
    }
    rejected(|workflow| workflow["jobs"]["browser-e2e"]["strategy"]["max-parallel"] = json!(100));
}

#[test]
fn docker_runtime_cannot_be_replaced_skipped_or_suppressed() {
    for (job, name) in [
        (
            "docker",
            "Verify image architecture, healthcheck and Web runtime",
        ),
        (
            "multiarch",
            "Verify both architectures boot with the real healthcheck",
        ),
    ] {
        for (key, value) in [
            ("run", json!("true")),
            ("if", json!("false")),
            ("continue-on-error", json!(true)),
        ] {
            rejected(|workflow| {
                let steps = workflow["jobs"][job]["steps"]
                    .as_array_mut()
                    .expect("steps");
                steps
                    .iter_mut()
                    .find(|step| step["name"] == name)
                    .expect("runtime")[key] = value;
            });
        }
    }
}

#[test]
fn multiarch_must_load_both_matching_images_for_runtime_verification() {
    for arch in ["amd64", "arm64"] {
        let name = format!("Load {arch} image from cache");
        rejected(|workflow| {
            workflow["jobs"]["multiarch"]["steps"]
                .as_array_mut()
                .expect("steps")
                .retain(|step| step["name"] != name);
        });
        for (key, value) in [
            ("platforms", json!("linux/other")),
            ("load", json!(false)),
            ("push", json!(true)),
            ("tags", json!("wrong-image")),
        ] {
            rejected(|workflow| {
                let steps = workflow["jobs"]["multiarch"]["steps"]
                    .as_array_mut()
                    .expect("steps");
                steps
                    .iter_mut()
                    .find(|step| step["name"] == name)
                    .expect("loader")["with"][key] = value;
            });
        }
    }
}

#[test]
fn installer_smoke_cannot_skip_or_suppress_failures() {
    for (key, value) in [
        ("if", json!("false")),
        ("run", json!("true")),
        ("continue-on-error", json!(true)),
    ] {
        rejected(|workflow| {
            let steps = workflow["jobs"]["browser-e2e"]["steps"]
                .as_array_mut()
                .expect("steps");
            let step = steps
                .iter_mut()
                .find(|step| step["name"] == "Verify native installer lifecycle")
                .expect("installer step");
            step[key] = value;
        });
    }
}
