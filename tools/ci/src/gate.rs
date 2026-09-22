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
    "docker",
    "multiarch",
];

pub(crate) fn evaluate(needs: &Value, event: &str) -> Value {
    let required: BTreeSet<_> = REQUIRED_JOBS.iter().copied().collect();
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
        "schema_version": 3, "profile": "full", "jobs": results,
        "status": if errors.is_empty() { "pass" } else { "fail" },
        "errors": errors, "case_execution": "not asserted by job aggregation",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
