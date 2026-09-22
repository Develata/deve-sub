use crate::gate::REQUIRED_JOBS;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;

pub(crate) const CONCURRENCY_GROUP: &str = "ci-${{ github.workflow }}-${{ github.event_name == 'pull_request' && github.event.pull_request.number || github.run_id }}";
pub(crate) const CANCEL_PR: &str = "${{ github.event_name == 'pull_request' }}";
const BROWSER_SUITES: &[(&str, &str)] = &[
    ("legacy", "--config playwright.config.ts"),
    (
        "functional-api",
        "--config functional.config.ts --project functional-api --workers 2",
    ),
    (
        "functional-desktop",
        "--config functional.config.ts --project functional-desktop --workers 2",
    ),
    (
        "functional-mobile",
        "--config functional.config.ts --project functional-mobile --workers 2",
    ),
];

pub(crate) fn workspace(metadata: &Value) -> Result<BTreeSet<String>> {
    let members = array(&metadata["workspace_members"])?;
    let mut names = BTreeSet::new();
    for package in array(&metadata["packages"])? {
        if members.contains(&package["id"]) {
            names.insert(string(&package["name"])?.to_owned());
        }
    }
    ensure!(!names.is_empty(), "empty workspace inventory");
    ensure!(
        names.len() == members.len(),
        "incomplete workspace metadata"
    );
    Ok(names)
}

pub(crate) fn validate(workflow: &Value, packages: &BTreeSet<String>) -> Result<usize> {
    reject_overrides(workflow)?;
    ensure!(
        workflow["concurrency"]
            == json!({
                "group": CONCURRENCY_GROUP, "cancel-in-progress": CANCEL_PR,
            }),
        "only obsolete runs of the same PR may be cancelled"
    );
    let jobs = workflow["jobs"]
        .as_object()
        .context("workflow jobs must be an object")?;
    let required: BTreeSet<_> = REQUIRED_JOBS.iter().copied().collect();
    let expected: BTreeSet<_> = required
        .iter()
        .copied()
        .chain(["acceptance-gate"])
        .collect();
    ensure!(
        jobs.keys().map(String::as_str).collect::<BTreeSet<_>>() == expected,
        "workflow jobs differ from the required full inventory"
    );
    for (name, job) in jobs {
        ensure!(
            job["timeout-minutes"]
                .as_u64()
                .is_some_and(|v| (1..=90).contains(&v)),
            "{name}: explicit job timeout must be between 1 and 90 minutes"
        );
        ensure!(
            job.get("continue-on-error").is_none(),
            "{name}: job cannot suppress errors"
        );
        if name != "multiarch" && name != "acceptance-gate" {
            ensure!(
                job.get("if").is_none(),
                "{name}: baseline job cannot be conditional"
            );
        }
    }
    let gate = &jobs["acceptance-gate"];
    let needs = array(&gate["needs"])?;
    ensure!(
        needs.iter().map(string).collect::<Result<BTreeSet<_>>>()? == required
            && needs.len() == required.len()
            && gate["if"] == "always()",
        "final gate must always depend on every required job exactly once"
    );
    ensure!(
        jobs["multiarch"]["if"] == "github.event_name != 'pull_request'",
        "only PR multiarch may be skipped"
    );
    let count = rust_shards(&jobs["test"], packages)?;
    browser_lanes(&jobs["browser-e2e"])?;
    Ok(count)
}

fn rust_shards(test: &Value, packages: &BTreeSet<String>) -> Result<usize> {
    reject_overrides(test)?;
    let rows = matrix(test)?;
    let commands: Vec<_> = array(&test["steps"])?
        .iter()
        .filter(|step| step.get("run").is_some())
        .collect();
    ensure!(
        commands.len() == 1,
        "Rust shard must have exactly one command"
    );
    ensure!(
        commands[0]
            == &json!({
                "name": "Run full Rust shard", "env": {"CI_PACKAGES": "${{ matrix.crates }}"},
                "run": "# shellcheck disable=SC2086\ncargo test --locked --all-targets --all-features $CI_PACKAGES\n",
            }),
        "Rust shard must run the exact unfiltered full Cargo command"
    );
    let mut shards = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for row in rows {
        let shard = string(&row["shard"])?;
        ensure!(
            valid_name(shard) && shards.insert(shard),
            "invalid or duplicate shard: {shard}"
        );
        let tokens: Vec<_> = string(&row["crates"])?.split_whitespace().collect();
        ensure!(
            !tokens.is_empty() && tokens.len().is_multiple_of(2),
            "invalid package list: {shard}"
        );
        for pair in tokens.chunks_exact(2) {
            ensure!(
                pair[0] == "-p" && valid_name(pair[1]),
                "invalid package token: {shard}"
            );
            ensure!(
                seen.insert(pair[1].to_owned()),
                "duplicate package owner: {}",
                pair[1]
            );
        }
    }
    ensure!(
        &seen == packages,
        "shard coverage mismatch: missing={:?}, extra={:?}",
        packages.difference(&seen).collect::<Vec<_>>(),
        seen.difference(packages).collect::<Vec<_>>()
    );
    Ok(shards.len())
}

fn browser_lanes(browser: &Value) -> Result<()> {
    reject_overrides(browser)?;
    let rows = matrix(browser)?;
    ensure!(
        browser["strategy"]["max-parallel"] == 4,
        "browser lanes must be bounded at four"
    );
    let expected: BTreeSet<_> = BROWSER_SUITES.iter().copied().collect();
    let actual: BTreeSet<_> = rows
        .iter()
        .map(|row| Ok((string(&row["suite"])?, string(&row["args"])?)))
        .collect::<Result<_>>()?;
    ensure!(
        rows.len() == expected.len() && actual == expected,
        "browser suite partition is incomplete or altered"
    );
    let steps = array(&browser["steps"])?;
    let command = steps
        .iter()
        .find(|step| step["name"] == "Run browser lane")
        .context("browser lane command missing")?;
    ensure!(
        command
            == &json!({
                "name": "Run browser lane", "env": {"CI_BROWSER_ARGS": "${{ matrix.args }}"},
                "run": "# shellcheck disable=SC2086\nnpx playwright test $CI_BROWSER_ARGS\n", "working-directory": "tests/e2e",
            }),
        "browser command cannot be filtered, skipped or error-suppressed"
    );
    let lifecycle = steps
        .iter()
        .find(|step| step["name"] == "Verify browser process lifecycle")
        .context("lifecycle command missing")?;
    ensure!(
        lifecycle
            == &json!({
                "name": "Verify browser process lifecycle", "if": "matrix.suite == 'legacy'",
                "run": "npx playwright test --config lifecycle.config.ts", "working-directory": "tests/e2e",
            }),
        "legacy lane must also verify browser process lifecycle"
    );
    Ok(())
}

fn matrix(job: &Value) -> Result<&Vec<Value>> {
    ensure!(
        job["strategy"]["fail-fast"] == false,
        "all matrix lanes must finish for complete feedback"
    );
    let matrix = job["strategy"]["matrix"]
        .as_object()
        .context("missing static matrix")?;
    ensure!(
        matrix.len() == 1 && matrix.contains_key("include"),
        "only the complete static include matrix is supported"
    );
    let rows = array(&matrix["include"])?;
    ensure!(!rows.is_empty(), "empty matrix");
    Ok(rows)
}

fn reject_overrides(value: &Value) -> Result<()> {
    ensure!(
        value.get("env").is_none() && value.get("defaults").is_none(),
        "baseline cannot override command environment or working directory"
    );
    Ok(())
}

fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn array(value: &Value) -> Result<&Vec<Value>> {
    value.as_array().context("expected array")
}

fn string(value: &Value) -> Result<&str> {
    value.as_str().context("expected string")
}
