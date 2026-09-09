#!/usr/bin/env python3
"""Small Cargo/layer and source-size gate; includes tracked and untracked files."""
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    errors = []
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version=1", "--no-deps", "--locked"], cwd=ROOT
    ))
    for package in metadata["packages"]:
        name = package["name"]
        production = {dep["name"] for dep in package["dependencies"] if dep["kind"] != "dev"}
        local = {dep for dep in production if dep.startswith("deve-sub-")}
        if name == "deve-sub-domain":
            forbidden = (local - {"deve-sub-kernel"}) | (production & {"sqlx", "axum", "reqwest", "anyhow"})
        elif name == "deve-sub-server":
            forbidden = local - {"deve-sub-domain", "deve-sub-kernel", "deve-sub-application", "deve-sub-contract", "deve-sub-compatibility", "deve-sub-security", "deve-sub-web"}
            forbidden |= production & {"sqlx", "reqwest"}
        elif name == "deve-sub-contract":
            forbidden = local | (production & {"sqlx", "axum", "tokio", "reqwest"})
            utoipa = next((d for d in package["dependencies"] if d["name"] == "utoipa"), None)
            if utoipa and not utoipa["optional"]:
                errors.append("contract: utoipa must remain optional for WASM")
        else:
            forbidden = set()
        if forbidden:
            errors.append(f"{name}: forbidden production dependencies {sorted(forbidden)}")

    exceptions = json.loads((ROOT / "scripts/architecture-exceptions.json").read_text())
    files = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=ROOT
    ).decode().split("\0")
    encountered = set()
    for name in set(files):
        path = ROOT / name
        if not name.endswith(".rs") or not path.is_file() or not name.startswith(("apps/", "crates/")):
            continue
        text = path.read_text()
        if name.startswith("apps/server/src/") and re.search(r"State\s*<\s*AppState\s*>", text):
            errors.append(f"{name}: handlers must extract capability-scoped State")
        if name.startswith(("crates/deve-sub-domain/src/", "crates/deve-sub-application/src/")) and "as_db_char" in text:
            errors.append(f"{name}: database discriminants belong to storage")
        if "/tests/" in name or "/benches/" in name or path.stem.endswith("_tests"):
            continue
        lines = len(text.splitlines())
        exception = exceptions.get(name)
        if exception:
            encountered.add(name)
            if not exception.get("reason") or exception["max_lines"] < 501:
                errors.append(f"{name}: invalid size exception")
        limit = exception["max_lines"] if exception else 500
        if lines > limit:
            errors.append(f"{name}: {lines} lines exceeds reviewed limit {limit}")
        if exception and lines <= 500:
            errors.append(f"{name}: remove obsolete size exception")
    for stale in exceptions.keys() - encountered:
        errors.append(f"{stale}: stale size exception")
    for workflow in (ROOT / ".github/workflows").glob("*.yml"):
        for ref in re.findall(r"uses:\s+(\S+)", workflow.read_text()):
            if not ref.startswith("./") and not re.fullmatch(r"[^@]+@[0-9a-f]{40}", ref):
                errors.append(f"{workflow.name}: action is not SHA-pinned: {ref}")
    if errors:
        raise SystemExit("\n".join(errors))
    print(f"architecture: {len(metadata['packages'])} crates checked; layers, optional OpenAPI, scoped state, SHA pins and source fuse pass")


if __name__ == "__main__":
    main()
