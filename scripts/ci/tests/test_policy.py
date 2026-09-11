"""Adversarial checks for static command scope and fail-closed full aggregation."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import yaml
from common import ROOT
from gate import evaluate
from inventory import REQUIRED_JOBS, shards, workspace


class PolicyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"], cwd=ROOT))
        cls.owners = workspace(cls.metadata)
        cls.workflow = yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text())
        cls.matrix = yaml.safe_load((ROOT / "tests/acceptance/matrix.yaml").read_text())

    def test_full_partition_includes_every_member_once(self):
        partitions = shards(self.workflow, self.owners)
        self.assertEqual(sum(map(len, partitions.values())), len(self.owners))

    def test_missing_and_duplicate_package_block(self):
        for mutation in ("missing", "duplicate"):
            workflow = copy.deepcopy(self.workflow)
            rows = workflow["jobs"]["test"]["strategy"]["matrix"]["include"]
            if mutation == "missing":
                rows.pop()
            else:
                rows[0]["crates"] += " " + rows[-1]["crates"]
            with self.assertRaises(ValueError):
                shards(workflow, self.owners)

    def test_final_gate_cannot_omit_a_leaf(self):
        workflow = copy.deepcopy(self.workflow)
        workflow["jobs"]["acceptance-gate"]["needs"].remove("compatibility")
        with self.assertRaises(ValueError):
            shards(workflow, self.owners)

    def test_replaced_filtered_or_suppressed_command_blocks(self):
        for mutation in ("replace", "filter", "skip", "suppress", "extra"):
            workflow = copy.deepcopy(self.workflow)
            job = workflow["jobs"]["test"]
            step = next(s for s in job["steps"] if "run" in s)
            if mutation == "replace":
                step["run"] = "true"
            elif mutation == "filter":
                step["run"] += " only_one_test"
            elif mutation == "skip":
                step["if"] = "false"
            elif mutation == "suppress":
                step["continue-on-error"] = True
            else:
                step["run"] += " || true"
            with self.assertRaises(ValueError):
                shards(workflow, self.owners)

    def test_failures_skips_cancellation_and_missing_jobs_block(self):
        passing = {job: {"result": "success"} for job in REQUIRED_JOBS}
        self.assertEqual(evaluate(passing, "push")["status"], "pass")
        for state in ("failure", "skipped", "cancelled", "timed_out", "unknown"):
            for job in REQUIRED_JOBS:
                needs = copy.deepcopy(passing)
                needs[job]["result"] = state
                self.assertEqual(evaluate(needs, "push")["status"], "fail")
        del passing["inventory"]
        self.assertEqual(evaluate(passing, "push")["status"], "fail")

    def test_only_pr_multiarch_skip_is_allowed_and_reported_not_run(self):
        needs = {job: {"result": "success"} for job in REQUIRED_JOBS}
        needs["multiarch"]["result"] = "skipped"
        report = evaluate(needs, "pull_request")
        self.assertEqual(report["status"], "pass")
        self.assertEqual(report["jobs"]["multiarch"]["status"], "not-run")
        for event in ("push", "schedule", "workflow_call", "workflow_dispatch"):
            self.assertEqual(evaluate(needs, event)["status"], "fail")



if __name__ == "__main__":
    unittest.main()
