#!/usr/bin/env python3
"""Docs and contracts validation for Deve Sub.

Runs in CI as the `docs` job. Checks:
  1. Acceptance matrix YAML parses and cases are well-formed.
  2. Acceptance matrix TSV row count is consistent with the YAML.
  3. Markdown files under docs/ have balanced code fences (Mermaid guard).
  4. Coverage matrix exists and is non-empty.
  5. OpenAPI spec: if present, is valid JSON; if absent, report pending.

This script depends on PyYAML (`pip install pyyaml`).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

try:
    import yaml
except ImportError:
    print(
        "PyYAML not installed. Run: pip install pyyaml",
        file=sys.stderr,
    )
    sys.exit(2)

ROOT = Path(__file__).resolve().parent.parent
MATRIX_YAML = ROOT / "tests" / "acceptance" / "matrix.yaml"
MATRIX_TSV = ROOT / "docs" / "acceptance" / "matrix.tsv"
COVERAGE = ROOT / "docs" / "coverage-matrix.md"
OPENAPI = ROOT / "docs" / "openapi" / "openapi.json"
DOCS_DIR = ROOT / "docs"

REQUIRED_CASE_FIELDS = {"id", "title", "priority", "layer", "evidence"}


def check_matrix_yaml() -> int:
    if not MATRIX_YAML.is_file():
        print(f"FAIL: acceptance matrix not found: {MATRIX_YAML}", file=sys.stderr)
        return 1
    with MATRIX_YAML.open(encoding="utf-8") as f:
        data = yaml.safe_load(f)
    if not isinstance(data, dict):
        print("FAIL: matrix.yaml top-level is not a mapping", file=sys.stderr)
        return 1
    cases = data.get("cases")
    if not isinstance(cases, list):
        print("FAIL: matrix.yaml 'cases' is not a list", file=sys.stderr)
        return 1
    ids: set[str] = set()
    for idx, case in enumerate(cases):
        if not isinstance(case, dict):
            print(f"FAIL: case #{idx} is not a mapping", file=sys.stderr)
            return 1
        missing = REQUIRED_CASE_FIELDS - case.keys()
        if missing:
            print(
                f"FAIL: case #{idx} ({case.get('id', '?')}) missing fields: {missing}",
                file=sys.stderr,
            )
            return 1
        cid = case["id"]
        if cid in ids:
            print(f"FAIL: duplicate case id: {cid}", file=sys.stderr)
            return 1
        ids.add(cid)
    print(f"OK: matrix.yaml parsed with {len(cases)} cases, all unique, all required fields present.")
    return 0


def check_matrix_tsv(yaml_count: int) -> int:
    if not MATRIX_TSV.is_file():
        print(f"FAIL: matrix.tsv not found: {MATRIX_TSV}", file=sys.stderr)
        return 1
    lines = MATRIX_TSV.read_text(encoding="utf-8").splitlines()
    # Drop trailing empty line if present.
    while lines and lines[-1].strip() == "":
        lines.pop()
    header = lines[0].split("\t") if lines else []
    expected_header = ["id", "title", "priority", "layer", "status"]
    if header != expected_header:
        print(
            f"FAIL: matrix.tsv header {header} != expected {expected_header}",
            file=sys.stderr,
        )
        return 1
    tsv_rows = len(lines) - 1
    if tsv_rows != yaml_count:
        print(
            f"FAIL: matrix.tsv has {tsv_rows} data rows but matrix.yaml has {yaml_count} cases",
            file=sys.stderr,
        )
        return 1
    print(f"OK: matrix.tsv has {tsv_rows} rows, consistent with YAML.")
    return 0


def check_code_fences() -> int:
    failures = 0
    md_files = sorted(DOCS_DIR.rglob("*.md"))
    for md in md_files:
        text = md.read_text(encoding="utf-8")
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


def check_coverage() -> int:
    if not COVERAGE.is_file():
        print(f"FAIL: coverage matrix not found: {COVERAGE}", file=sys.stderr)
        return 1
    size = COVERAGE.stat().st_size
    if size == 0:
        print("FAIL: coverage matrix is empty.", file=sys.stderr)
        return 1
    print(f"OK: coverage-matrix.md present ({size} bytes).")
    return 0


def check_openapi() -> int:
    if not OPENAPI.is_file():
        print("OK: docs/openapi/openapi.json not present yet (pending utoipa export, M2+).")
        return 0
    try:
        json.loads(OPENAPI.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        print(f"FAIL: openapi.json is not valid JSON: {exc}", file=sys.stderr)
        return 1
    print("OK: openapi.json is valid JSON.")
    return 0


def main() -> int:
    exit_code = 0
    print("=== Acceptance matrix (YAML) ===")
    exit_code |= check_matrix_yaml()
    # Re-read count for TSV cross-check without re-parsing fully.
    yaml_count = 0
    if MATRIX_YAML.is_file():
        with MATRIX_YAML.open(encoding="utf-8") as f:
            data = yaml.safe_load(f)
        if isinstance(data, dict) and isinstance(data.get("cases"), list):
            yaml_count = len(data["cases"])
    print("=== Acceptance matrix (TSV) ===")
    exit_code |= check_matrix_tsv(yaml_count)
    print("=== Markdown code fences ===")
    exit_code |= check_code_fences()
    print("=== Coverage matrix ===")
    exit_code |= check_coverage()
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
