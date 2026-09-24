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


class ReleasePolicyTests(unittest.TestCase):
    def test_semver_prerelease_classification_and_tag_mismatch(self):
        jobs = yaml.safe_load((ROOT / ".github/workflows/release.yml").read_text())["jobs"]
        step = next(s for s in jobs["version-check"]["steps"] if s.get("id") == "version")
        for version, expected in [("1.2.3", "false"), ("1.2.3-rc.1", "true")]:
            with self.subTest(version=version), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / "Cargo.toml").write_text(f'[workspace.package]\nversion = "{version}"\n')
                output = root / "output"
                env = {**os.environ, "GITHUB_REF_NAME": "v" + version,
                       "GITHUB_EVENT_NAME": "push", "GITHUB_OUTPUT": str(output)}
                result = subprocess.run(["bash", "-euo", "pipefail", "-c", step["run"]],
                                        cwd=root, env=env, capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output.read_text(), f"prerelease={expected}\n")
                env["GITHUB_REF_NAME"] = "v0.0.0"
                result = subprocess.run(["bash", "-euo", "pipefail", "-c", step["run"]],
                                        cwd=root, env=env, capture_output=True, text=True, timeout=5)
                self.assertNotEqual(result.returncode, 0)
        release = jobs["release"]
        self.assertIn("version-check", release["needs"])
        publish = next(s for s in release["steps"] if s.get("name") == "Create GitHub Release")
        self.assertEqual(publish["with"]["prerelease"],
                         "${{ needs.version-check.outputs.prerelease == 'true' }}")

    def test_oci_incompatible_build_metadata_fails_before_publication(self):
        jobs = yaml.safe_load((ROOT / ".github/workflows/release.yml").read_text())["jobs"]
        step = next(s for s in jobs["version-check"]["steps"] if s.get("id") == "version")
        for event in ("push", "workflow_dispatch"):
            for version in ("1.2.3+build.1", "1.2.3-rc.1+build.1"):
                with self.subTest(event=event, version=version), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    (root / "Cargo.toml").write_text(f'[workspace.package]\nversion = "{version}"\n')
                    output = root / "output"
                    env = {**os.environ, "GITHUB_REF_NAME": "v" + version,
                           "GITHUB_EVENT_NAME": event, "GITHUB_OUTPUT": str(output)}
                    result = subprocess.run(["bash", "-euo", "pipefail", "-c", step["run"]],
                                            cwd=root, env=env, capture_output=True, text=True, timeout=5)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("cannot contain build metadata", result.stdout)
                    self.assertFalse(output.exists())

    def test_manual_candidate_upload_has_only_complete_release_assets(self):
        jobs = yaml.safe_load((ROOT / ".github/workflows/release.yml").read_text())["jobs"]
        steps = jobs["release"]["steps"]
        upload = next(s for s in steps if s.get("name") == "Upload release candidate")
        verify = next(s for s in steps if s.get("name") == "Verify candidate asset completeness")
        publish = next(s for s in steps if s.get("name") == "Create GitHub Release")
        expected = {
            "deve-sub-linux-amd64", "deve-sub-linux-arm64", "deve-sub-web.tar.gz",
            "checksums.txt", "deve-sub-manifest.json", "deve-sub-manifest.json.sig",
            "deve-sub-sbom.json", "deve-sub-web-sbom.json",
        }
        paths = upload["with"]["path"].splitlines()
        self.assertEqual(len(paths), len(expected))
        self.assertEqual(set(paths), {"${{ runner.temp }}/release/" + a for a in expected})
        self.assertEqual(set(paths), set(publish["with"]["files"].splitlines()))
        self.assertEqual(upload["uses"],
                         "actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02")
        self.assertEqual(upload["with"]["name"], "release-candidate")
        self.assertEqual(upload["with"]["if-no-files-found"], "error")
        self.assertTrue(1 <= upload["with"]["retention-days"] <= 7)
        for step in (verify, upload):
            self.assertEqual(step["if"], "github.event_name == 'workflow_dispatch'")
        signing = next(s for s in steps if s.get("name") == "Sign release manifest")
        sbom = next(s for s in steps if s.get("name") == "Generate SBOM")
        self.assertLess(steps.index(signing), steps.index(verify))
        self.assertLess(steps.index(sbom), steps.index(verify))
        self.assertLess(steps.index(verify), steps.index(upload))
        # upload-artifact's no-files policy does not reject one missing path.
        # Execute the completeness guard for every partial/empty asset set.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            release = root / "release"
            release.mkdir()
            for asset in expected:
                (release / asset).write_bytes(b"fixture")
            env = {**os.environ, "RUNNER_TEMP": str(root)}

            def check():
                return subprocess.run(["bash", "-euo", "pipefail", "-c", verify["run"]],
                                      env=env, capture_output=True, text=True, timeout=5)

            self.assertEqual(check().returncode, 0)
            for asset in expected:
                with self.subTest(asset=asset):
                    path = release / asset
                    path.unlink()
                    self.assertNotEqual(check().returncode, 0)
                    path.touch()
                    self.assertNotEqual(check().returncode, 0)
                    path.write_bytes(b"fixture")

    def test_release_execution_jobs_have_bounded_deadlines(self):
        jobs = yaml.safe_load((ROOT / ".github/workflows/release.yml").read_text())["jobs"]
        for name, job in jobs.items():
            if "uses" not in job:  # Reusable CI declares its own job deadlines.
                with self.subTest(job=name):
                    self.assertTrue(1 <= job["timeout-minutes"] <= 60)


if __name__ == "__main__":
    unittest.main()
