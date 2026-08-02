#!/usr/bin/env python3
"""Docs and contracts validation for Deve Sub.

Runs in CI as the `docs` job. Checks (per docs/acceptance/gates.md):
  1. matrix.yaml parses with required fields and unique case IDs.
  2. matrix.tsv has the same case IDs as matrix.yaml (set equality).
  3. Markdown files under docs/ have balanced code fences.
  4. coverage-matrix.md tokens match matrix IDs (wildcards like PARSE-*
     require ≥1 matching ID; specific IDs like UI-008 must exist).
  5. OpenAPI spec: if present, is valid JSON; if absent, report pending.

This script depends on PyYAML (`pip install pyyaml`).
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

try:
    import yaml
except ImportError:
    print("PyYAML not installed. Run: pip install pyyaml", file=sys.stderr)
    sys.exit(2)

ROOT = Path(__file__).resolve().parent.parent
MATRIX_YAML = ROOT / "tests" / "acceptance" / "matrix.yaml"
MATRIX_TSV = ROOT / "docs" / "acceptance" / "matrix.tsv"
COVERAGE = ROOT / "docs" / "coverage-matrix.md"
OPENAPI = ROOT / "docs" / "openapi" / "openapi.json"
DOCS_DIR = ROOT / "docs"

REQUIRED_CASE_FIELDS = {"id", "title", "priority", "layer", "evidence"}
TOKEN_RE = re.compile(r"(?<![A-Z])([A-Z]+-(?:\*|\d+))")


def _read_text(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError as exc:
        print(f"FAIL: {path} is not valid UTF-8: {exc}", file=sys.stderr)
        return None


def _load_yaml(path: Path) -> dict | None:
    try:
        with path.open(encoding="utf-8") as f:
            data = yaml.safe_load(f)
    except yaml.YAMLError as exc:
        print(f"FAIL: {path} is not valid YAML: {exc}", file=sys.stderr)
        return None
    if not isinstance(data, dict):
        print(f"FAIL: {path} top-level is not a mapping", file=sys.stderr)
        return None
    return data


def check_matrix_yaml() -> tuple[int, set[str]]:
    """Return (exit_code, case_ids)."""
    if not MATRIX_YAML.is_file():
        print(f"FAIL: acceptance matrix not found: {MATRIX_YAML}", file=sys.stderr)
        return 1, set()
    data = _load_yaml(MATRIX_YAML)
    if data is None:
        return 1, set()
    cases = data.get("cases")
    if not isinstance(cases, list):
        print("FAIL: matrix.yaml 'cases' is not a list", file=sys.stderr)
        return 1, set()
    if not cases:
        print("FAIL: matrix.yaml 'cases' is empty", file=sys.stderr)
        return 1, set()
    ids: set[str] = set()
    for idx, case in enumerate(cases):
        if not isinstance(case, dict):
            print(f"FAIL: case #{idx} is not a mapping", file=sys.stderr)
            return 1, ids
        missing = REQUIRED_CASE_FIELDS - case.keys()
        if missing:
            print(
                f"FAIL: case #{idx} ({case.get('id', '?')}) missing fields: {missing}",
                file=sys.stderr,
            )
            return 1, ids
        cid = case["id"]
        if not isinstance(cid, str):
            print(f"FAIL: case #{idx} id is not a string: {cid!r}", file=sys.stderr)
            return 1, ids
        if cid in ids:
            print(f"FAIL: duplicate case id: {cid}", file=sys.stderr)
            return 1, ids
        ids.add(cid)
    print(f"OK: matrix.yaml parsed with {len(cases)} cases, all unique, all required fields present.")
    return 0, ids


def check_matrix_tsv(yaml_ids: set[str]) -> int:
    if not MATRIX_TSV.is_file():
        print(f"FAIL: matrix.tsv not found: {MATRIX_TSV}", file=sys.stderr)
        return 1
    text = _read_text(MATRIX_TSV)
    if text is None:
        return 1
    lines = text.splitlines()
    while lines and lines[-1].strip() == "":
        lines.pop()
    if not lines:
        print("FAIL: matrix.tsv is empty.", file=sys.stderr)
        return 1
    header = lines[0].split("\t")
    expected_header = ["id", "title", "priority", "layer", "status"]
    if header != expected_header:
        print(
            f"FAIL: matrix.tsv header {header} != expected {expected_header}",
            file=sys.stderr,
        )
        return 1
    tsv_ids: list[str] = []
    for lineno, line in enumerate(lines[1:], start=2):
        cols = line.split("\t")
        if len(cols) != len(expected_header):
            print(
                f"FAIL: matrix.tsv line {lineno} has {len(cols)} columns, expected {len(expected_header)}",
                file=sys.stderr,
            )
            return 1
        tsv_ids.append(cols[0])
    tsv_set = set(tsv_ids)
    if len(tsv_ids) != len(tsv_set):
        dupes = [i for i in tsv_ids if tsv_ids.count(i) > 1]
        print(f"FAIL: matrix.tsv has duplicate IDs: {set(dupes)}", file=sys.stderr)
        return 1
    if tsv_set != yaml_ids:
        only_tsv = tsv_set - yaml_ids
        only_yaml = yaml_ids - tsv_set
        print(
            f"FAIL: matrix.tsv IDs do not match matrix.yaml IDs."
            f" Only in TSV: {sorted(only_tsv)}. Only in YAML: {sorted(only_yaml)}.",
            file=sys.stderr,
        )
        return 1
    print(f"OK: matrix.tsv has {len(tsv_ids)} IDs, all match matrix.yaml.")
    return 0


def check_code_fences() -> int:
    failures = 0
    md_files = sorted(DOCS_DIR.rglob("*.md"))
    for md in md_files:
        text = _read_text(md)
        if text is None:
            failures += 1
            continue
        fence_count = text.count("```")
        if fence_count % 2 != 0:
            print(
                f"FAIL: unbalanced code fences ({fence_count} ticks) in {md.relative_to(ROOT)}",
                file=sys.stderr,
            )
            failures += 1
    if failures == 0:
        print(f"OK: {len(md_files)} markdown files have balanced code fences.")
    return 1 if failures else 0


def check_coverage(matrix_ids: set[str]) -> int:
    if not COVERAGE.is_file():
        print(f"FAIL: coverage matrix not found: {COVERAGE}", file=sys.stderr)
        return 1
    text = _read_text(COVERAGE)
    if text is None:
        return 1
    if not text.strip():
        print("FAIL: coverage matrix is empty.", file=sys.stderr)
        return 1
    tokens = set(TOKEN_RE.findall(text))
    if not tokens:
        print("FAIL: no acceptance tokens found in coverage-matrix.md.", file=sys.stderr)
        return 1
    failures = 0
    for token in sorted(tokens):
        if token.endswith("-*"):
            prefix = token[:-1]  # keep trailing "-"
            matches = [i for i in matrix_ids if i.startswith(prefix)]
            if not matches:
                print(
                    f"FAIL: coverage token {token} has no matching IDs in matrix (prefix {prefix!r}).",
                    file=sys.stderr,
                )
                failures += 1
        else:
            if token not in matrix_ids:
                print(
                    f"FAIL: coverage token {token} not found in matrix IDs.",
                    file=sys.stderr,
                )
                failures += 1
    if failures == 0:
        print(f"OK: coverage-matrix.md has {len(tokens)} tokens, all match matrix IDs.")
    return 1 if failures else 0


def check_openapi() -> int:
    if not OPENAPI.is_file():
        print("OK: docs/openapi/openapi.json not present yet (pending utoipa export, M2+).")
        return 0
    text = _read_text(OPENAPI)
    if text is None:
        return 1
    try:
        json.loads(text)
    except json.JSONDecodeError as exc:
        print(f"FAIL: openapi.json is not valid JSON: {exc}", file=sys.stderr)
        return 1
    print("OK: openapi.json is valid JSON.")
    return 0


def main() -> int:
    exit_code = 0
    print("=== Acceptance matrix (YAML) ===")
    rc, yaml_ids = check_matrix_yaml()
    exit_code |= rc
    print("=== Acceptance matrix (TSV) ===")
    exit_code |= check_matrix_tsv(yaml_ids)
    print("=== Markdown code fences ===")
    exit_code |= check_code_fences()
    print("=== Coverage matrix ===")
    exit_code |= check_coverage(yaml_ids)
    print("=== OpenAPI ===")
    exit_code |= check_openapi()
    print("=== Summary ===")
    if exit_code == 0:
        print("All docs checks passed.")
    else:
        print("One or more docs checks failed.", file=sys.stderr)
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
