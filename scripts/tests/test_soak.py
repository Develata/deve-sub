"""Regression: an aborted real-process harness must retain a non-pass report."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class SoakFailureTests(unittest.TestCase):
    def test_startup_exit_retains_failure_report(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / 'exits-on-serve'
            binary.write_text('#!/bin/sh\n[ "$1" = migrate ] && exit 0\nexit 7\n')
            binary.chmod(0o755)
            report = root / 'report.json'
            result = subprocess.run([
                'python3', str(ROOT / 'scripts/perf/soak.py'), '--binary', str(binary),
                '--seconds', '10', '--output', str(report),
            ], capture_output=True, text=True, timeout=45)
            self.assertNotEqual(result.returncode, 0)
            value = json.loads(report.read_text())
            self.assertEqual(value['status'], 'FAIL')
            self.assertEqual(value['failure'], 'serve exited during startup')
            self.assertEqual(value['samples'], [])


if __name__ == '__main__':
    unittest.main()
