"""Execute the release alias step against isolated registry/API doubles."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[3]


class LatestImageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.job = yaml.safe_load((ROOT / ".github/workflows/release.yml").read_text())[
            "jobs"
        ]["docker-release"]
        cls.step = next(s for s in cls.job["steps"] if s.get("name") ==
                        "Promote current stable release to latest")

    def run_promotion(self, tag="v1.2.3", latest="v1.2.3", api_exit=0,
                      docker_exit=0, digest="sha256:" + "a" * 64):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            gh = root / "gh"
            gh.write_text("#!/bin/sh\nprintf '%s\\n' \"$FIXTURE_LATEST\"\n"
                          "exit \"$FIXTURE_API_EXIT\"\n")
            docker = root / "docker"
            docker.write_text("#!/usr/bin/env python3\nimport json, os, sys\n"
                              "from pathlib import Path\n"
                              "Path(os.environ['FIXTURE_CALL']).write_text(json.dumps(sys.argv[1:]))\n"
                              "sys.exit(int(os.environ['FIXTURE_DOCKER_EXIT']))\n")
            gh.chmod(0o755)
            docker.chmod(0o755)
            call = root / "call.json"
            env = {**os.environ, "PATH": f"{root}:{os.environ['PATH']}",
                   "GITHUB_REPOSITORY": "Example/Example", "GITHUB_REF_NAME": tag,
                   "GH_TOKEN": "fixture", "PUBLISHED_DIGEST": digest,
                   "FIXTURE_LATEST": latest, "FIXTURE_API_EXIT": str(api_exit),
                   "FIXTURE_DOCKER_EXIT": str(docker_exit), "FIXTURE_CALL": str(call)}
            result = subprocess.run(["bash", "-euo", "pipefail", "-c", self.step["run"]],
                                    env=env, capture_output=True, text=True, timeout=5)
            return result.returncode, json.loads(call.read_text()) if call.exists() else None

    def test_current_stable_copies_the_exact_published_digest(self):
        code, call = self.run_promotion()
        self.assertEqual(code, 0)
        self.assertEqual(call, ["buildx", "imagetools", "create", "--tag",
                               "ghcr.io/example/example:latest",
                               "ghcr.io/example/example@sha256:" + "a" * 64])

    def test_prerelease_and_old_release_never_write_alias(self):
        for tag, latest in [("v1.2.3-rc.1", "v1.2.3-rc.1"),
                            ("v1.2.2", "v1.2.3"), ("main", "main")]:
            with self.subTest(tag=tag):
                self.assertEqual(self.run_promotion(tag, latest), (0, None))

    def test_failed_latest_query_and_missing_digest_never_write_alias(self):
        for args in [{"api_exit": 1}, {"digest": ""}]:
            with self.subTest(args=args):
                code, call = self.run_promotion(**args)
                self.assertNotEqual(code, 0)
                self.assertIsNone(call)

    def test_registry_failure_fails_the_step(self):
        code, call = self.run_promotion(docker_exit=1)
        self.assertNotEqual(code, 0)
        self.assertIsNotNone(call)

    def test_publication_still_requires_tag_release_and_serializes_writers(self):
        self.assertEqual(self.job["if"], "github.event_name == 'push'")
        self.assertEqual(self.job["needs"], "release")
        self.assertFalse(self.job["concurrency"]["cancel-in-progress"])
        self.assertEqual(self.job["concurrency"]["queue"], "max")
        self.assertNotIn("ref", self.job["concurrency"]["group"])
        publish = next(s for s in self.job["steps"] if s.get("id") == "publish")
        self.assertLess(self.job["steps"].index(publish), self.job["steps"].index(self.step))
        self.assertIn("steps.publish.outputs.digest", self.step["env"]["PUBLISHED_DIGEST"])


if __name__ == "__main__":
    unittest.main()
