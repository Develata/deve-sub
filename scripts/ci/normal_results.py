"""Strict coverage/result checks for the reviewed normal regression catalogue."""
import json
import os

from common import ROOT, identity

CATALOGUE = ROOT / "tests/acceptance/normal-ci.json"


def catalogue():
    value = json.loads(CATALOGUE.read_text())
    if value.get("schema_version") != 1 or not value.get("lanes"):
        raise ValueError("missing normal catalogue")
    all_cases = []
    for name, lane in value["lanes"].items():
        if not lane.get("cases") or lane["kind"] not in ("playwright", "dns"):
            raise ValueError(f"{name}: empty or unknown lane")
        all_cases.extend(lane["cases"])
    if len(all_cases) != len(set(all_cases)):
        raise ValueError("duplicate catalogue ownership")
    return value


def context():
    return {
        **identity(),
        "run_id": os.environ.get("GITHUB_RUN_ID", "local"),
        "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT", "local"),
    }


def specs(report):
    """Keep every test occurrence, so duplicate reports cannot collapse away."""
    result = []

    def visit(suite):
        for spec in suite.get("specs", []):
            for test in spec["tests"]:
                key = f"{spec['file']}::{spec['title']}::{test['projectName']}"
                result.append((key, spec, test))
        for child in suite.get("suites", []):
            visit(child)

    visit(report)
    return result


def exact(actual, expected, label):
    if len(actual) != len(set(actual)) or set(actual) != set(expected):
        raise ValueError(f"{label}: duplicate/missing/unknown cases; "
                         f"missing={sorted(set(expected) - set(actual))}; "
                         f"extra={sorted(set(actual) - set(expected))}")


def playwright_results(report, expected):
    rows = specs(report)
    exact([row[0] for row in rows], expected, "executed coverage")
    if report.get("errors"):
        raise ValueError("Playwright reported a global failure")
    cases = []
    for key, spec, test in rows:
        results = test.get("results", [])
        passed = (spec.get("ok") is True and test.get("expectedStatus") == "passed"
                  and test.get("status") == "expected" and len(results) == 1
                  and results[0].get("status") == "passed"
                  and results[0].get("retry") == 0
                  and not results[0].get("errors"))
        cases.append({"id": key, "status": "PASS" if passed else "FAIL",
                      "observed": [r.get("status") for r in results]})
    if any(report.get("stats", {}).get(key, 0) for key in ("unexpected", "flaky", "skipped")):
        raise ValueError("Playwright failure, retry or skip statistics")
    return cases


def dns_results(proof, binary_sha256, case_id):
    if (proof.get("status") != "PASS" or proof.get("external_connections") is not False
            or [row["address_type"] for row in proof.get("observations", [])] != [1, 3]
            or len(proof.get("commands", [])) != 6
            or any(row["exit_code"] != 0 for row in proof["commands"])
            or proof.get("binary_sha256") != binary_sha256):
        raise ValueError("DNS handshake evidence incomplete")
    return [{"id": case_id, "status": "PASS"}]


def evaluate(reports, plan, expected_context, needs):
    """GitHub completion and exact per-case evidence are both required."""
    errors, cases = [], []
    expected_jobs = {"normal-acceptance", "build-release", "web-wasm", "rust-gate"}
    if set(needs) != expected_jobs:
        errors.append("required producer/matrix job inventory differs")
    for job in expected_jobs:
        outcome = needs.get(job, {})
        status = outcome.get("result", "missing") if isinstance(outcome, dict) else "malformed"
        if status != "success":
            errors.append(f"{job}: {status}")
    names = [r.get("lane") for r in reports]
    if len(names) != len(set(names)) or set(names) != set(plan["lanes"]):
        errors.append("missing, duplicate or unknown normal lane report")
    for report in reports:
        lane = report.get("lane")
        if lane not in plan["lanes"]:
            continue
        if type(report.get("schema_version")) is not int or report["schema_version"] != 1 or report.get("context") != expected_context:
            errors.append(f"{lane}: stale source/run or incomplete schema")
        if (report.get("status") != "PASS" or type(report.get("exit_code")) is not int
                or report["exit_code"] != 0 or report.get("errors")):
            errors.append(f"{lane}: execution did not pass")
        rows = report.get("cases", [])
        if not isinstance(rows, list) or any(not isinstance(row, dict) or not isinstance(row.get("id"), str) for row in rows):
            errors.append(f"{lane}: malformed case results")
            rows = []
        try:
            exact([r["id"] for r in rows], plan["lanes"][lane]["cases"], lane)
        except (ValueError, KeyError) as error:
            errors.append(str(error))
        for row in rows:
            if row.get("status") != "PASS":
                errors.append(f"{lane}: {row.get('id')} {row.get('status', 'missing')}")
        cases.extend(rows)
    expected = [case for lane in plan["lanes"].values() for case in lane["cases"]]
    try:
        exact([case["id"] for case in cases], expected, "complete normal coverage")
    except (ValueError, KeyError) as error:
        errors.append(str(error))
    return {
        "schema_version": 1, "scope": plan["scope"], "context": expected_context,
        "status": "FAIL" if errors else "PASS", "errors": errors,
        "expected_lanes": len(plan["lanes"]), "received_lanes": len(reports),
        "expected_cases": len(expected), "cases": cases, "jobs": needs,
        "external_scope": plan["external_scope"],
        "full_acceptance_readiness": "NOT_ASSERTED",
    }
