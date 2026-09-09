"""Reject stale/tampered receipts and exercise actual child-process outcomes."""
import contextlib
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import gate
from common import write_json
from execution import package_args, run_context, scope, validate, verify_collection
from run_shard import execute, run
from inventory import REQUIRED_JOBS

SOURCE = {"commit": "a" * 40, "source_digest": "b" * 64, "dirty": False,
          "toolchain": "1.94.0", "lockfile_digest": "c" * 64}
RUN = {"run_id": "123", "run_attempt": "2"}
PACKAGES = ["deve-sub-domain"]
COMPILER = "rustc 1.94.0 (fixture 2026-03-01)\nhost: x86_64-unknown-linux-gnu\n"


def passing():
    return {**scope("core", PACKAGES, SOURCE, RUN),
            "compiler": COMPILER.splitlines()[0], "compiler_host": "x86_64-unknown-linux-gnu",
            "started_at": "2026-09-09T12:00:00+00:00", "finished_at": "2026-09-09T12:00:01+00:00",
            "elapsed_ms": 1000, "exit_code": 0, "status": "pass", "diagnostic": None}


class ReceiptTests(unittest.TestCase):
    def test_final_gate_requires_receipts_even_with_successful_job_fixture(self):
        # Synthetic job results test aggregation; they are not remote CI evidence.
        env = {"CI_NEEDS": json.dumps({job: {"result": "success"} for job in REQUIRED_JOBS}),
               "GITHUB_EVENT_NAME": "push", "GITHUB_RUN_ID": RUN["run_id"],
               "GITHUB_RUN_ATTEMPT": RUN["run_attempt"]}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "result.json"
            receipts = root / "receipts"
            args = ["gate.py", "--receipts", str(receipts), "--output", str(output)]
            for complete in (False, True):
                if complete:
                    write_json(receipts / "rust-execution-core/receipt.json", passing())
                with patch.dict(os.environ, env, clear=True), patch.object(sys, "argv", args), \
                     patch("gate.identity", return_value=SOURCE), \
                     patch("gate.matrix_shards", return_value={"core": PACKAGES}), \
                     contextlib.redirect_stdout(io.StringIO()):
                    if complete:
                        gate.main()
                    else:
                        with self.assertRaises(SystemExit) as failure:
                            gate.main()
                        self.assertEqual(failure.exception.code, 1)
                result = json.loads(output.read_text())
                self.assertEqual(result["schema_version"], 2)
                self.assertEqual(result["status"], "pass" if complete else "fail")

    def test_scope_and_execution_tampering_is_rejected(self):
        validate(passing(), "core", PACKAGES, SOURCE, RUN)
        changes = {"commit": "d" * 40, "source_digest": "e" * 64, "dirty": 0,
                   "toolchain": "1.93.0", "lockfile_digest": "f" * 64,
                   "run_id": "124", "run_attempt": "1", "shard": "server",
                   "packages": [], "command": ["true"], "profile": "release",
                   "target": "wasm32-unknown-unknown", "schema_version": True,
                   "kind": "other", "exit_code": False, "status": "running",
                   "diagnostic": "failed", "elapsed_ms": True,
                   "compiler": "rustc 1.93.0 (fixture)", "compiler_host": "aarch64-unknown-linux-gnu",
                   "started_at": "2026-09-09T12:00:02+00:00", "finished_at": "2026-09-09T12:00:01"}
        for key, value in changes.items():
            with self.subTest(key=key):
                receipt = passing()
                receipt[key] = value
                with self.assertRaises(ValueError):
                    validate(receipt, "core", PACKAGES, SOURCE, RUN)
        for state in ("fail", "cancelled", "not-run", "planned", "blocked"):
            receipt = passing()
            receipt["status"] = state
            with self.assertRaises(ValueError):
                validate(receipt, "core", PACKAGES, SOURCE, RUN)

    def test_inventory_schema_and_filesystem_are_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "rust-execution-core/receipt.json"
            write_json(path, passing())
            def check():
                return verify_collection(root, {"core": PACKAGES}, SOURCE, RUN)[1]
            self.assertEqual(check(), [])
            original = path.read_text()
            for malformed in (original.replace('"exit_code": 0', '"exit_code": 0, "exit_code": 0'),
                              original.replace('"elapsed_ms": 1000', '"elapsed_ms": NaN'),
                              "[]", "{}", "x" * 65537):
                path.write_text(malformed)
                self.assertTrue(check())
            path.write_text(original)
            extra = root / "unexpected"
            extra.mkdir()
            self.assertTrue(check())
            extra.rmdir()
            extra = path.parent / "extra.json"
            extra.write_text("{}")
            self.assertTrue(check())
            extra.unlink()
            target = root / "outside.json"
            path.rename(target)
            path.symlink_to(target)
            self.assertTrue(check())
            path.unlink()
            target.unlink()
            self.assertTrue(check())
            self.assertTrue(verify_collection(root / "absent", {"core": PACKAGES}, SOURCE, RUN)[1])

    def test_input_cannot_extend_cargo_command(self):
        self.assertEqual(package_args("-p deve-sub-domain -p deve-sub-application"),
                         ["deve-sub-domain", "deve-sub-application"])
        for value in ("", "--workspace", "-p deve-sub-domain --ignored", "-p bad;command",
                      "-p deve-sub-domain -p deve-sub-domain", "-p ../outside"):
            with self.assertRaises(ValueError):
                package_args(value)
        self.assertEqual(run_context(SOURCE, {}), {"run_id": "local", "run_attempt": "local"})
        for env in ({"GITHUB_RUN_ID": "1"}, {"GITHUB_RUN_ATTEMPT": "1"}, {"GITHUB_SHA": "other"}):
            with self.assertRaises(ValueError):
                run_context(SOURCE, env)


class RunnerTests(unittest.TestCase):
    def test_actual_child_exit_and_missing_executable(self):
        for code in (0, 7):
            self.assertEqual(execute([sys.executable, "-c", f"raise SystemExit({code})"], Path.cwd()), (code, False))
        with self.assertRaises(FileNotFoundError):
            execute(["/nonexistent/deve-sub-test-command"], Path.cwd())

    def test_receipt_preserves_failure_cancellation_and_source_change(self):
        for outcome, changed, expected in (((0, False), False, "pass"), ((7, False), False, "fail"),
                                           ((143, True), False, "cancelled"), ((0, False), True, "fail")):
            with self.subTest(outcome=outcome, changed=changed), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "receipt.json"
                after = {**SOURCE, "source_digest": "f" * 64} if changed else SOURCE
                with patch.dict(os.environ, {}, clear=True), patch("run_shard.identity", side_effect=[SOURCE, after]), \
                     patch("run_shard.subprocess.check_output", return_value=COMPILER), \
                     patch("run_shard.execute", return_value=outcome):
                    code = run("core", PACKAGES, output)
                receipt = json.loads(output.read_text())
                self.assertEqual(receipt["status"], expected)
                self.assertEqual(receipt["exit_code"], outcome[0])
                self.assertEqual(code == 0, expected == "pass")
                self.assertIsNotNone(receipt["finished_at"])

    def test_compiler_or_spawn_failure_never_leaves_pass(self):
        for compiler, error in (("rustc wrong", None), (COMPILER, FileNotFoundError("test executable absent"))):
            with tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "receipt.json"
                with patch.dict(os.environ, {}, clear=True), patch("run_shard.identity", return_value=SOURCE), \
                     patch("run_shard.subprocess.check_output", return_value=compiler), \
                     patch("run_shard.execute", side_effect=error):
                    self.assertEqual(run("core", PACKAGES, output), 1)
                receipt = json.loads(output.read_text())
                self.assertEqual(receipt["status"], "fail")
                self.assertIsNone(receipt["exit_code"])
                self.assertIsNotNone(receipt["diagnostic"])

    def test_sigterm_cleans_child_group_even_when_parent_exits_first(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            child_script = root / "child.py"
            child_script.write_text(
                "import os, pathlib, signal, subprocess, sys, time\n"
                "desc = subprocess.Popen([sys.executable, '-c', "
                "'import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(60)'])\n"
                "time.sleep(0.2)\n"
                "pathlib.Path('pids').write_text(str(os.getpid())+' '+str(desc.pid))\n"
                "time.sleep(60)\n")
            wrapper = root / "wrapper.py"
            wrapper.write_text(
                f"import sys\nsys.path.insert(0, {str(Path(__file__).resolve().parents[1])!r})\n"
                "from run_shard import execute\nfrom pathlib import Path\n"
                "code, cancelled = execute([sys.executable, 'child.py'], Path.cwd())\n"
                "assert cancelled\nraise SystemExit(code)\n")
            process = subprocess.Popen([sys.executable, str(wrapper)], cwd=root)
            pids = []
            try:
                deadline = time.monotonic() + 10
                while not (root / "pids").exists() and time.monotonic() < deadline:
                    time.sleep(0.05)
                self.assertTrue((root / "pids").exists(), "child did not start")
                pids = [int(pid) for pid in (root / "pids").read_text().split()]
                process.send_signal(signal.SIGTERM)
                self.assertEqual(process.wait(timeout=10), 143)
                deadline = time.monotonic() + 2
                def active(pid):
                    try:
                        return Path(f"/proc/{pid}/stat").read_text().split()[2] != "Z"
                    except FileNotFoundError:
                        return False
                while any(active(pid) for pid in pids) and time.monotonic() < deadline:
                    time.sleep(0.05)
                self.assertFalse(any(active(pid) for pid in pids), "cancelled command left a live descendant")
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                for pid in pids:
                    try:
                        os.kill(pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass


if __name__ == "__main__":
    unittest.main()
