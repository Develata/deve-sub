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
VALID_EVIDENCE_STATUS = {"pass", "fail", "planned", "not-run", "blocked"}
VALID_EVIDENCE = VALID_EVIDENCE_STATUS  # legacy scalar form
VALID_PRIORITY_RE = re.compile(r"^P\d+$")
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
        evidence = case["evidence"]
        if isinstance(evidence, dict):
            status = evidence.get("status")
            if status not in VALID_EVIDENCE_STATUS:
                print(
                    f"FAIL: case {cid} has invalid evidence.status {status!r}; expected one of {sorted(VALID_EVIDENCE_STATUS)}",
                    file=sys.stderr,
                )
                return 1, ids
            tests = evidence.get("tests")
            if not isinstance(tests, list):
                print(
                    f"FAIL: case {cid} evidence.tests is not a list",
                    file=sys.stderr,
                )
                return 1, ids
        elif isinstance(evidence, str):
            if evidence not in VALID_EVIDENCE:
                print(
                    f"FAIL: case {cid} has invalid evidence {evidence!r}; expected one of {sorted(VALID_EVIDENCE)}",
                    file=sys.stderr,
                )
                return 1, ids
        else:
            print(
                f"FAIL: case {cid} evidence must be a string or a mapping, got {type(evidence).__name__}",
                file=sys.stderr,
            )
            return 1, ids
        priority = case["priority"]
        if not VALID_PRIORITY_RE.match(str(priority)):
            print(
                f"FAIL: case {cid} has invalid priority {priority!r}; expected P<n>",
                file=sys.stderr,
            )
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
        cid, _title, priority, _layer, status = cols
        if not VALID_PRIORITY_RE.match(priority):
            print(
                f"FAIL: matrix.tsv line {lineno} has invalid priority {priority!r}; expected P<n>",
                file=sys.stderr,
            )
            return 1
        if status not in VALID_EVIDENCE:
            print(
                f"FAIL: matrix.tsv line {lineno} ({cid}) has invalid status {status!r}; expected one of {sorted(VALID_EVIDENCE)}",
                file=sys.stderr,
            )
            return 1
        tsv_ids.append(cid)
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
        in_fence = False
        for line in text.splitlines():
            if line.lstrip().startswith("```"):
                in_fence = not in_fence
        if in_fence:
            print(
                f"FAIL: unbalanced code fences in {md.relative_to(ROOT)}",
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


def check_test_symbols(yaml_data: dict) -> int:
    """Verify that test symbols referenced in evidence exist on disk."""
    failures = 0
    checked = 0
    rust_fn_re = re.compile(r"\bfn\s+(\w+)\s*\(")
    ts_test_re = re.compile(r"""test\s*\(\s*['"].*?(\w[\w-]*)""")
    for case in yaml_data.get("cases", []):
        ev = case.get("evidence", {})
        if not isinstance(ev, dict):
            continue
        if ev.get("status") != "pass":
            continue
        tests = ev.get("tests", [])
        for ref in tests:
            if not isinstance(ref, str) or "::" not in ref:
                continue
            path_str, symbol = ref.rsplit("::", 1)
            checked += 1
            path = ROOT / path_str
            if not path.is_file():
                print(f"FAIL: {case['id']} references missing file: {path_str}", file=sys.stderr)
                failures += 1
                continue
            text = _read_text(path)
            if text is None:
                failures += 1
                continue
            if path_str.endswith(".rs"):
                symbols = set(rust_fn_re.findall(text))
                if symbol not in symbols:
                    print(
                        f"FAIL: {case['id']} references missing symbol '{symbol}' in {path_str}",
                        file=sys.stderr,
                    )
                    failures += 1
            elif path_str.endswith((".ts", ".js")):
                ts_names = set(ts_test_re.findall(text))
                if symbol not in ts_names:
                    print(
                        f"FAIL: {case['id']} references missing test '{symbol}' in {path_str}",
                        file=sys.stderr,
                    )
                    failures += 1
    if failures == 0:
        print(f"OK: {checked} test symbol references verified.")
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
    if rc == 0:
        print("=== Test symbol references ===")
        data = _load_yaml(MATRIX_YAML)
        if data is not None:
            exit_code |= check_test_symbols(data)
        else:
            print("SKIP: YAML reload failed.", file=sys.stderr)
            exit_code |= 1
        print("=== Acceptance matrix (TSV) ===")
        exit_code |= check_matrix_tsv(yaml_ids)
        print("=== Coverage matrix ===")
        exit_code |= check_coverage(yaml_ids)
    else:
        print("=== Acceptance matrix (TSV) ===")
        print("SKIP: YAML parse failed; TSV comparison skipped.")
        print("=== Coverage matrix ===")
        print("SKIP: YAML parse failed; coverage check skipped.")
    print("=== Markdown code fences ===")
    exit_code |= check_code_fences()
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
