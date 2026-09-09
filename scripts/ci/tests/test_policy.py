"""Adversarial checks for impact closure and fail-closed full aggregation."""
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
from inventory import REQUIRED_JOBS, cases, shards, workspace
from plan import changed_paths, propose


class PolicyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"], cwd=ROOT))
        cls.owners, cls.reverse = workspace(cls.metadata)
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

    def test_reverse_dev_cycles_are_included_without_infinite_loop(self):
        selected = propose(["crates/deve-sub-emitter/src/lib.rs"], self.owners, self.reverse, {}, {})
        self.assertIn("deve-sub-protocol", selected["packages"])
        selected = propose(["crates/deve-sub-storage-sqlite/src/lib.rs"], self.owners, self.reverse, {}, {})
        self.assertIn("deve-sub-application", selected["packages"])
        self.assertIn("deve-sub-server", selected["packages"])

    def test_optional_target_and_build_consumers_are_included(self):
        metadata = copy.deepcopy(self.metadata)
        packages = {p["name"]: p for p in metadata["packages"]}
        target = packages["deve-sub-observability"]
        for kind in (None, "dev", "build"):
            packages["deve-sub-web"]["dependencies"].append({
                "name": target["name"], "path": str(Path(target["manifest_path"]).parent),
                "kind": kind, "optional": True, "target": "cfg(target_family = \"wasm\")"})
        owners, reverse = workspace(metadata)
        selected = propose(["crates/deve-sub-observability/src/lib.rs"], owners, reverse, {}, {})
        self.assertIn("deve-sub-web", selected["packages"])

    def test_unknown_and_authority_changes_propose_full(self):
        for path in ("unknown.txt", "Cargo.lock", "migrations/0001.sql", "docs/contracts/new.md", "apps/web/Cargo.toml", "apps/cli/src/backup_database.rs", "crates/deve-sub-application/src/auth/commands.rs"):
            selected = propose([path], self.owners, self.reverse, {}, {})
            self.assertEqual(selected["profile"], "full")
            self.assertEqual(set(selected["packages"]), set(self.owners))

    def test_registered_not_run_remains_not_run(self):
        registered = cases(self.matrix, self.owners)
        self.assertEqual(registered["PERF-001"]["historical_status"], "not-run")
        self.assertTrue(all(case["execution_status"] == "not-run" for case in registered.values()))

    def test_missing_case_proof_blocks_inventory(self):
        matrix = copy.deepcopy(self.matrix)
        matrix["cases"][0]["evidence"] = {"status": "pass", "tests": ["nonexistent.rs::missing"]}
        with self.assertRaises(ValueError):
            cases(matrix, self.owners)

    def test_failures_skips_cancellation_and_missing_jobs_block(self):
        passing = {job: {"result": "success"} for job in REQUIRED_JOBS}
        self.assertEqual(evaluate(passing, "push")["status"], "pass")
        for state in ("failure", "skipped", "cancelled", "timed_out", "unknown"):
            for job in REQUIRED_JOBS:
                needs = copy.deepcopy(passing)
                needs[job]["result"] = state
                self.assertEqual(evaluate(needs, "push")["status"], "fail")
        del passing["plan"]
        self.assertEqual(evaluate(passing, "push")["status"], "fail")

    def test_only_pr_multiarch_skip_is_allowed_and_reported_not_run(self):
        needs = {job: {"result": "success"} for job in REQUIRED_JOBS}
        needs["multiarch"]["result"] = "skipped"
        report = evaluate(needs, "pull_request")
        self.assertEqual(report["status"], "pass")
        self.assertEqual(report["jobs"]["multiarch"]["status"], "not-run")
        for event in ("push", "schedule", "workflow_call", "workflow_dispatch"):
            self.assertEqual(evaluate(needs, event)["status"], "fail")

    def test_diff_includes_rename_old_path_and_untracked(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, stderr=subprocess.DEVNULL)
            git("init", "-q")
            (root / "old.rs").write_text("example\n")
            git("add", "old.rs")
            git("-c", "user.name=CI test", "-c", "user.email=ci@example.invalid", "commit", "-qm", "fixture")
            (root / "old.rs").rename(root / "new.rs")
            (root / "untracked.rs").write_text("new\n")
            paths, reason = changed_paths("HEAD", root)
            self.assertIsNone(reason)
            self.assertEqual(paths, ["new.rs", "old.rs", "untracked.rs"])
            self.assertIsNotNone(changed_paths("missing-ref", root)[1])


if __name__ == "__main__":
    unittest.main()
