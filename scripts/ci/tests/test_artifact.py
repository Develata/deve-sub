"""Reject stale, incomplete and tampered artifact handoffs."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from artifact import MANIFEST, create, verify
from common import digest

SOURCE = {"commit": "a" * 40, "source_digest": "b" * 64, "dirty": False,
          "toolchain": "1.97.1", "lockfile_digest": "c" * 64}


class ArtifactTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "deve-sub").write_bytes(b"synthetic binary")
        create(self.root, "binary", SOURCE, "rustc 1.97.1 (fixture)")
        self.manifest_sha256 = digest(self.root / MANIFEST)

    def test_exact_handoff_passes(self):
        verify(self.root, "binary", SOURCE, self.manifest_sha256)

    def test_changed_missing_extra_and_symlinked_files_fail(self):
        binary = self.root / "deve-sub"
        binary.write_bytes(b"tampered binary!")
        with self.assertRaises(ValueError):
            verify(self.root, "binary", SOURCE, self.manifest_sha256)
        binary.unlink()
        with self.assertRaises(ValueError):
            verify(self.root, "binary", SOURCE, self.manifest_sha256)
        binary.symlink_to("/dev/null")
        with self.assertRaises(ValueError):
            verify(self.root, "binary", SOURCE, self.manifest_sha256)
        binary.unlink()
        binary.write_bytes(b"synthetic binary")
        (self.root / "unrelated-report.json").write_text("{}")
        with self.assertRaises(ValueError):
            verify(self.root, "binary", SOURCE, self.manifest_sha256)

    def test_context_mismatch_and_incomplete_manifest_fail(self):
        for field in ("commit", "source_digest", "toolchain", "lockfile_digest"):
            with self.assertRaises(ValueError):
                verify(self.root, "binary", {**SOURCE, field: "different"}, self.manifest_sha256)
        path = self.root / MANIFEST
        manifest = json.loads(path.read_text())
        manifest["run_attempt"] = "wrong-run"
        path.write_text(json.dumps(manifest))
        with self.assertRaises(ValueError):
            verify(self.root, "binary", SOURCE, self.manifest_sha256)
        del manifest["files"]
        path.write_text(json.dumps(manifest))
        with self.assertRaises(ValueError):
            verify(self.root, "binary", SOURCE, self.manifest_sha256)

    def test_replacement_payload_and_manifest_cannot_self_certify(self):
        (self.root / "deve-sub").write_bytes(b"replacement binary")
        create(self.root, "binary", SOURCE, "rustc 1.97.1 (fixture)")
        with self.assertRaises(ValueError):
            verify(self.root, "binary", SOURCE, self.manifest_sha256)
        with self.assertRaises(ValueError):
            verify(self.root, "binary", SOURCE, "")

    def test_wrong_compiler_is_rejected_at_production(self):
        with self.assertRaises(ValueError):
            create(self.root, "binary", SOURCE, "rustc 1.96.0 (fixture)")

    def test_wasm_requires_complete_dist_and_rejects_changed_assets(self):
        (self.root / "deve-sub").unlink()
        (self.root / "index.html").write_text("synthetic frontend")
        (self.root / "assets").mkdir()
        for extension in ("wasm", "js"):
            (self.root / "assets" / f"bundle.{extension}").write_text("fixture")
        with self.assertRaises(ValueError):
            create(self.root, "wasm", SOURCE, "rustc 1.97.1 (fixture)")
        (self.root / "assets/bundle.css").write_text("fixture")
        create(self.root, "wasm", SOURCE, "rustc 1.97.1 (fixture)")
        self.manifest_sha256 = digest(self.root / MANIFEST)
        verify(self.root, "wasm", SOURCE, self.manifest_sha256)
        (self.root / "assets/bundle.js").write_text("changed")
        with self.assertRaises(ValueError):
            verify(self.root, "wasm", SOURCE, self.manifest_sha256)


if __name__ == "__main__":
    unittest.main()
