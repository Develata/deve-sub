use super::*;

#[test]
fn native_pairs_cannot_be_missing_duplicated_or_redirected() {
    rejected(|w| {
        w["jobs"]["multiarch"]["strategy"]["matrix"]["include"]
            .as_array_mut()
            .expect("rows")
            .pop();
    });
    rejected(|w| {
        w["jobs"]["multiarch"]["strategy"]["matrix"]["include"][1] =
            w["jobs"]["multiarch"]["strategy"]["matrix"]["include"][0].clone();
    });
    for (key, value) in [
        ("runner", "ubuntu-latest"),
        ("platform", "linux/amd64"),
        ("machine", "x86_64"),
        ("arch", "amd64"),
    ] {
        rejected(|w| {
            w["jobs"]["multiarch"]["strategy"]["matrix"]["include"][1][key] = json!(value)
        });
    }
    rejected(|w| w["jobs"]["multiarch"]["runs-on"] = json!("ubuntu-latest"));
    rejected(|w| w["jobs"]["multiarch"]["strategy"]["fail-fast"] = json!(true));
    rejected(|w| w["jobs"]["multiarch"]["strategy"]["max-parallel"] = json!(3));
}

#[test]
fn native_steps_cannot_be_skipped_replaced_or_suppressed() {
    for job in ["docker", "multiarch"] {
        let len = workflow()["jobs"][job]["steps"]
            .as_array()
            .expect("steps")
            .len();
        for index in 1..len {
            for (key, value) in [
                ("run", json!("true")),
                ("if", json!("false")),
                ("continue-on-error", json!(true)),
            ] {
                rejected(|w| w["jobs"][job]["steps"][index][key] = value);
            }
        }
        for action in [
            "docker/setup-qemu-action@fixture",
            "docker/setup-buildx-action@fixture",
        ] {
            rejected(|w| {
                w["jobs"][job]["steps"]
                    .as_array_mut()
                    .expect("steps")
                    .push(json!({"uses": action}))
            });
        }
        rejected(|w| {
            w["jobs"][job]["steps"]
                .as_array_mut()
                .expect("steps")
                .push(json!({"run": "docker run --privileged fixture"}))
        });
        rejected(|w| {
            w["jobs"][job]["container"] = json!({"image":"fixture", "options":"--privileged"})
        });
        rejected(|w| w["jobs"][job]["timeout-minutes"] = json!(90));
    }
    rejected(|w| {
        w["jobs"]["multiarch"]["steps"][2]["env"]["NATIVE_PLATFORM"] = json!("linux/amd64")
    });
    rejected(|w| w["jobs"]["multiarch"]["steps"][3]["env"]["NATIVE_ARCH"] = json!("amd64"));
}

#[test]
fn browser_acquisition_is_forbidden_in_every_ci_job() {
    for job in [
        "http-api-lifecycle",
        "normal-non-browser",
        "installer-lifecycle",
    ] {
        rejected(|w| {
            w["jobs"][job]["steps"]
                .as_array_mut()
                .expect("steps")
                .push(json!({"run":"npx playwright install chromium"}))
        });
    }
    rejected(|w| {
        let step = w["jobs"]["http-api-lifecycle"]["steps"]
            .as_array_mut()
            .expect("steps")
            .iter_mut()
            .find(|s| s["run"] == "npm ci")
            .expect("dependencies");
        step.as_object_mut().expect("step").remove("env");
    });
}
