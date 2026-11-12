"""Failure injection: an incomplete normal run must never become green."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from normal_results import catalogue, evaluate, playwright_results
import normal

CONTEXT = {"commit": "candidate", "source_digest": "current", "run_id": "42", "run_attempt": "2"}
NEEDS = {name: {"result": "success"} for name in ("normal-acceptance", "build-release", "web-wasm", "rust-gate")}
PLAN = {
    "scope": "unit fixture", "external_scope": {"status": "NOT_RUN"},
    "lanes": {"one": {"cases": ["a"]}, "two": {"cases": ["b"]}},
}


def passing():
    return [{
        "schema_version": 1, "lane": lane, "context": CONTEXT.copy(),
        "status": "PASS", "exit_code": 0, "errors": [],
        "cases": [{"id": case, "status": "PASS"} for case in value["cases"]],
    } for lane, value in PLAN["lanes"].items()]


def raw_report():
    return {"errors": [], "stats": {"expected": 1, "unexpected": 0, "flaky": 0, "skipped": 0},
            "suites": [{"specs": [{"file": "normal.spec.ts", "title": "works", "ok": True,
                "tests": [{"projectName": "", "expectedStatus": "passed", "status": "expected",
                           "results": [{"status": "passed", "retry": 0, "errors": []}]}]}]}]}


class NormalGateTests(unittest.TestCase):
    def check_failed(self, reports, needs=NEEDS):
        self.assertEqual(evaluate(reports, PLAN, CONTEXT, needs)["status"], "FAIL")

    def test_complete_case_results_pass_but_do_not_certify_external_scope(self):
        result = evaluate(passing(), PLAN, CONTEXT, NEEDS)
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(result["full_acceptance_readiness"], "NOT_ASSERTED")
        self.assertEqual(result["external_scope"]["status"], "NOT_RUN")

    def test_every_failed_cancelled_missing_skipped_job_blocks(self):
        for name in NEEDS:
            for status in ("failure", "cancelled", "skipped", "timed_out", "unknown"):
                with self.subTest(name=name, status=status):
                    needs = copy.deepcopy(NEEDS)
                    needs[name]["result"] = status
                    self.check_failed(passing(), needs)
            needs = copy.deepcopy(NEEDS)
            del needs[name]
            self.check_failed(passing(), needs)

    def test_missing_duplicate_unknown_lane_blocks(self):
        for reports in ([], passing()[:1], passing() + passing()[:1],
                        passing() + [{**passing()[0], "lane": "unknown"}]):
            self.check_failed(reports)

    def test_malformed_job_outcomes_fail_closed(self):
        for value in (None, [], "success", True):
            needs = copy.deepcopy(NEEDS)
            needs["normal-acceptance"] = value
            self.check_failed(passing(), needs)

    def test_stale_source_workflow_attempt_and_commit_block(self):
        for key in CONTEXT:
            reports = passing()
            reports[0]["context"][key] = "previous"
            self.check_failed(reports)

    def test_exit_failure_and_partial_results_block(self):
        for code in (1, -15, None, False):
            reports = passing()
            reports[0]["exit_code"] = code
            self.check_failed(reports)

    def test_malformed_case_shapes_fail_closed(self):
        for value in (None, {}, ["PASS"], [{"id": ["a"], "status": "PASS"}]):
            reports = passing()
            reports[0]["cases"] = value
            self.check_failed(reports)
        for status in ("FAIL", "BLOCKED", "NOT_RUN", "SKIP", "cancelled"):
            reports = passing()
            reports[0]["cases"][0]["status"] = status
            self.check_failed(reports)
        for cases in ([], [{"id": "unknown", "status": "PASS"}],
                      [{"id": "a", "status": "PASS"}] * 2):
            reports = passing()
            reports[0]["cases"] = cases
            self.check_failed(reports)

    def test_global_failure_cannot_hide_behind_passing_case(self):
        report = raw_report()
        report["errors"] = [{"message": "worker failed after test"}]
        with self.assertRaises(ValueError):
            playwright_results(report, ["normal.spec.ts::works::"])

    def test_skipped_retry_and_expected_failure_are_not_passes(self):
        for mutation in ("skipped", "retry", "expected-failure", "empty", "duplicate"):
            report = raw_report()
            test = report["suites"][0]["specs"][0]["tests"][0]
            if mutation == "skipped":
                test["results"][0]["status"] = "skipped"
            elif mutation == "retry":
                test["results"].append({"status": "passed", "retry": 1})
            elif mutation == "expected-failure":
                test["expectedStatus"] = "failed"
            elif mutation == "empty":
                test["results"] = []
            else:
                report["suites"].append(copy.deepcopy(report["suites"][0]))
            if mutation == "duplicate":
                with self.assertRaises(ValueError):
                    playwright_results(report, ["normal.spec.ts::works::"])
            else:
                self.assertEqual(playwright_results(report, ["normal.spec.ts::works::"])[0]["status"], "FAIL")

    def test_catalogue_is_complete_unique_and_nonempty(self):
        plan = catalogue()
        self.assertEqual(len(plan["lanes"]), 11)
        cases = [case for lane in plan["lanes"].values() for case in lane["cases"]]
        self.assertGreaterEqual(len(cases), 40)
        self.assertEqual(len(cases), len(set(cases)))

    def test_missing_artifacts_write_failed_report(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "gate.json"
            with patch.object(normal, "context", return_value=CONTEXT), patch.dict("os.environ", {"CI_NORMAL_NEEDS": json.dumps(NEEDS)}):
                self.assertEqual(normal.aggregate(root, output), 1)
            result = json.loads(output.read_text())
            self.assertEqual(result["status"], "FAIL")
            self.assertGreater(len(result["errors"]), 0)

    def test_corrupt_artifact_is_not_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "summary.json").write_text("{broken")
            output = root / "gate.json"
            with patch.object(normal, "context", return_value=CONTEXT), patch.dict("os.environ", {"CI_NORMAL_NEEDS": json.dumps(NEEDS)}):
                self.assertEqual(normal.aggregate(root, output), 1)
            self.assertTrue(any("summary.json" in value for value in json.loads(output.read_text())["errors"]))

    def test_malformed_lane_and_raw_report_write_failed_aggregate(self):
        for lane, raw in (([], {}), ("protocol-1", [])):
            with self.subTest(lane=lane), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / "browser").mkdir()
                path = root / "browser/results.json"
                path.write_text(json.dumps(raw))
                (root / "command.log").write_text("synthetic malformed-result control\n")
                (root / "summary.json").write_text(json.dumps({
                    "lane": lane, "raw_report_sha256": normal.digest(path),
                }))
                output = root / "gate.json"
                with patch.object(normal, "context", return_value=CONTEXT), patch.dict("os.environ", {"CI_NORMAL_NEEDS": json.dumps(NEEDS)}):
                    self.assertEqual(normal.aggregate(root, output), 1)
                self.assertEqual(json.loads(output.read_text())["status"], "FAIL")


if __name__ == "__main__":
    unittest.main()
