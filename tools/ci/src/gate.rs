use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

// WHY: deletion from the workflow must fail instead of shrinking the baseline.
pub(crate) const REQUIRED_JOBS: &[&str] = &[
    "inventory",
    "fmt-check",
    "clippy",
    "test",
    "test-doc",
    "openapi-diff",
    "rust-gate",
    "compatibility",
    "docs",
    "web-wasm",
    "build-release",
    "resource-soak",
    "supply-chain",
    "browser-e2e",
    "normal-acceptance",
    "normal-gate",
    "docker",
    "multiarch",
];

pub(crate) const CI_REQUIRED_JOBS: &[&str] = &[
    "inventory",
    "fmt-check",
    "clippy",
    "test",
    "test-doc",
    "openapi-diff",
    "rust-gate",
    "compatibility",
    "docs",
    "web-wasm",
    "build-release",
    "resource-soak",
    "supply-chain",
    "http-api-lifecycle",
    "installer-lifecycle",
    "normal-non-browser",
    "normal-non-browser-gate",
    "docker",
    "multiarch",
];

pub(crate) fn evaluate(needs: &Value, event: &str) -> Value {
    evaluate_profile(needs, event, "full")
}

pub(crate) fn evaluate_profile(needs: &Value, event: &str, profile: &str) -> Value {
    let jobs = match profile {
        "full" => REQUIRED_JOBS,
        "ci-non-browser" => CI_REQUIRED_JOBS,
        _ => return json!({"status":"fail", "profile":profile, "errors":["unknown CI profile"]}),
    };

    let required: BTreeSet<_> = jobs.iter().copied().collect();
    let supplied: BTreeSet<_> = needs
        .as_object()
        .into_iter()
        .flat_map(|jobs| jobs.keys().map(String::as_str))
        .collect();
    let mut errors = Vec::new();
    let mut results = BTreeMap::new();
    if supplied != required {
        errors.push(format!(
            "job inventory mismatch: missing={:?}, extra={:?}",
            required.difference(&supplied).collect::<Vec<_>>(),
            supplied.difference(&required).collect::<Vec<_>>()
        ));
    }
    for job in required {
        let result = needs[job]["result"].as_str().unwrap_or("missing");
        let allowed_skip = job == "multiarch" && event == "pull_request" && result == "skipped";
        let outcome = if allowed_skip {
            json!({"status": "not-run", "reason": "multiarch is excluded on PR by the full baseline policy"})
        } else {
            if result != "success" {
                errors.push(format!("{job}: {result}"));
            }
            json!({"status": result})
        };
        results.insert(job, outcome);
    }
    json!({
        "schema_version": 3, "profile": profile,
        "full_acceptance_readiness": "NOT_ASSERTED",
        "local_browser_scope": if profile == "ci-non-browser" { "NOT_RUN: 151 primary browser cases require local evidence" } else { "job aggregation only" }, "jobs": results,
        "status": if errors.is_empty() { "pass" } else { "fail" },
        "errors": errors, "case_execution": "not asserted by job aggregation",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_profile_is_never_full_and_unknown_profiles_fail() {
        let needs: Value = CI_REQUIRED_JOBS
            .iter()
            .map(|j| ((*j).to_owned(), json!({"result":"success"})))
            .collect();
        assert_eq!(
            evaluate_profile(&needs, "push", "ci-non-browser")["status"],
            "pass"
        );
        assert_eq!(evaluate(&needs, "push")["status"], "fail");
        assert_eq!(evaluate_profile(&needs, "push", "other")["status"], "fail");
        for job in CI_REQUIRED_JOBS {
            for state in ["failure", "skipped", "cancelled", "unknown"] {
                let mut failed = needs.clone();
                failed[*job]["result"] = json!(state);
                assert_eq!(
                    evaluate_profile(&failed, "push", "ci-non-browser")["status"],
                    "fail"
                );
            }
        }
    }

    fn passing() -> Value {
        REQUIRED_JOBS
            .iter()
            .map(|job| ((*job).to_owned(), json!({"result": "success"})))
            .collect()
    }

    #[test]
    fn every_failed_cancelled_missing_or_unknown_job_blocks() {
        assert_eq!(evaluate(&passing(), "push")["status"], "pass");
        for state in ["failure", "skipped", "cancelled", "timed_out", "unknown"] {
            for job in REQUIRED_JOBS {
                let mut needs = passing();
                needs[*job]["result"] = json!(state);
                assert_eq!(evaluate(&needs, "push")["status"], "fail", "{job}: {state}");
            }
        }
        assert_eq!(evaluate(&json!({}), "push")["status"], "fail");
        let mut needs = passing();
        needs["unexpected"] = json!({"result": "success"});
        assert_eq!(evaluate(&needs, "push")["status"], "fail");
    }

    #[test]
    fn only_pr_multiarch_skip_is_reported_as_not_run() {
        let mut needs = passing();
        needs["multiarch"]["result"] = json!("skipped");
        let report = evaluate(&needs, "pull_request");
        assert_eq!(report["status"], "pass");
        assert_eq!(report["jobs"]["multiarch"]["status"], "not-run");
        for event in ["push", "schedule", "workflow_call", "workflow_dispatch"] {
            assert_eq!(evaluate(&needs, event)["status"], "fail");
        }
    }
}
