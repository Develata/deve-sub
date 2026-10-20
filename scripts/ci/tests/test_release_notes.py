"""Exercise the exact reviewed Release body selection without publishing."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib
import unittest
from urllib.parse import unquote, urlsplit

import yaml

ROOT = Path(__file__).resolve().parents[3]


class ReleaseNotesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.job = yaml.safe_load((ROOT / ".github/workflows/release.yml").read_text())[
            "jobs"
        ]["release"]
        cls.step = next(s for s in cls.job["steps"] if s.get("id") == "notes")

    def run_selection(self, root):
        output = root / "output"
        result = subprocess.run(
            ["bash", "-euo", "pipefail", "-c", self.step["run"]], cwd=root,
            env={**os.environ, "GITHUB_OUTPUT": str(output),
                 "GITHUB_REF_NAME": "chore/release-candidate"},
            capture_output=True, text=True, timeout=5,
        )
        return result, output.read_text() if output.exists() else ""

    def test_selects_exact_versioned_body_without_rewriting(self):
        for version in ("1.2.3", "1.2.3-rc.1"):
            with self.subTest(version=version), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / "Cargo.toml").write_text(f'[workspace.package]\nversion = "{version}"\n')
                path = root / f"docs/guides/release-v{version}.md"
                path.parent.mkdir(parents=True)
                text = f"# Deve Sub v{version} release notes\n\nReviewed **body**.\n"
                path.write_text(text)
                # A stale document must never be used as a fallback.
                (path.parent / "release-v0.0.1.md").write_text("Unrelated notes")
                result, output = self.run_selection(root)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output, f"body_path={path.relative_to(root).as_posix()}\n")
                self.assertEqual(path.read_bytes(), text.encode())

    def test_missing_empty_and_wrong_version_notes_stop_assembly(self):
        for text in (None, "", " \n", "# Deve Sub v1.2.3 release notes\n\n",
                     "# Deve Sub v0.0.1 release notes\n\nWrong version.\n"):
            with self.subTest(text=text), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / "Cargo.toml").write_text('[workspace.package]\nversion = "1.2.3"\n')
                path = root / "docs/guides/release-v1.2.3.md"
                path.parent.mkdir(parents=True)
                (path.parent / "release-v0.0.1.md").write_text("Stale fallback")
                if text is not None:
                    path.write_text(text)
                result, output = self.run_selection(root)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output, "")

    def test_publication_uses_only_the_reviewed_file(self):
        steps = self.job["steps"]
        publish = next(s for s in steps if s.get("name") == "Create GitHub Release")
        self.assertLess(steps.index(self.step), steps.index(publish))
        self.assertNotIn("if", self.step)  # Manual preflight validates the body too.
        self.assertEqual(publish["with"]["body_path"], "${{ steps.notes.outputs.body_path }}")
        self.assertIs(publish["with"]["generate_release_notes"], False)
        self.assertIs(publish["with"]["append_body"], False)
        self.assertNotIn("body", publish["with"])

    def test_current_notes_have_release_page_safe_links(self):
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        text = (ROOT / f"docs/guides/release-v{version}.md").read_text()
        # The pinned release action truncates at 124,999 UTF-16 code units.
        self.assertLess(len(text.encode("utf-16-le")) // 2, 125000)
        links = re.findall(r"\[[^\]]+\]\(([^)]+)\)", text)
        self.assertTrue(links)
        for link in links:
            with self.subTest(link=link):
                url = urlsplit(link)
                self.assertEqual(url.scheme, "https")
                self.assertTrue(url.netloc)
                if url.netloc == "github.com" and "/blob/" in url.path:
                    prefix = f"/Develata/deve-sub/blob/v{version}/"
                    self.assertTrue(url.path.startswith(prefix))
                    target = ROOT / unquote(url.path.removeprefix(prefix))
                    self.assertTrue(target.is_file(), target)
                    if url.fragment:
                        headings = re.findall(r"^#+ (.+)$", target.read_text(), re.MULTILINE)
                        anchors = {re.sub(r"[^\w -]", "", h.lower()).replace(" ", "-")
                                   for h in headings}
                        self.assertIn(unquote(url.fragment), anchors)


if __name__ == "__main__":
    unittest.main()
