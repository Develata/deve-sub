#!/usr/bin/env python3
"""Small Cargo/layer and source-size gate; includes tracked and untracked files."""
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Production edges are owned by docs/contracts/module-boundaries.md. Dev-only
# adapters remain valid for integration tests; target-specific and build edges
# must obey the same boundaries as other production dependencies.
LAYERS = {
    "kernel": set(),
    "contract": set(),
    "domain": {"kernel"},
    "security": {"kernel"},
    "observability": {"kernel"},
    "protocol": {"kernel", "domain"},
    "emitter": {"kernel", "domain"},
    "compatibility": {"kernel", "domain"},
    "application": {"kernel", "contract", "domain", "security", "protocol", "emitter", "compatibility"},
    "storage-sqlite": {"kernel", "domain", "application", "security"},
    "inmemory": {"kernel", "domain", "application", "security"},
    "adapters": {"kernel", "domain", "application", "security"},
    "server": {"kernel", "contract", "domain", "application", "security", "compatibility", "web"},
    "web": {"contract"},
    "cli": {"kernel", "contract", "domain", "application", "security", "protocol", "emitter", "compatibility", "storage-sqlite", "inmemory", "adapters", "observability", "server", "web"},
    "ci": set(),
}
FRAMEWORKS = {"sqlx", "axum", "reqwest", "dioxus"}


def dependency_errors(packages):
    errors = []
    for package in packages:
        name = package["name"]
        layer = name.removeprefix("deve-sub-")
        if not name.startswith("deve-sub-") or layer not in LAYERS:
            errors.append(f"{name}: unclassified workspace package; declare its boundary")
            continue
        production = {dep["name"] for dep in package["dependencies"] if dep["kind"] != "dev"}
        local = {dep for dep in production if dep.startswith("deve-sub-")}
        forbidden = local - {f"deve-sub-{allowed}" for allowed in LAYERS[layer]}
        if layer in {"kernel", "contract", "domain", "protocol", "emitter", "compatibility", "application"}:
            forbidden |= production & FRAMEWORKS
        if layer in {"kernel", "contract", "domain"}:
            forbidden |= production & {"tokio"}
        if layer == "server":
            forbidden |= production & {"sqlx", "reqwest"}
        if layer == "web":
            forbidden |= production & {"sqlx", "axum", "reqwest"}
        if layer not in {"cli", "ci"}:
            forbidden |= production & {"anyhow"}
        if layer == "contract":
            if any(d["name"] == "utoipa" and d["kind"] != "dev" and not d["optional"]
                   for d in package["dependencies"]):
                errors.append("contract: utoipa must remain optional for WASM")
        if forbidden:
            errors.append(f"{name}: forbidden production dependencies {sorted(forbidden)}")
    return errors


def main():
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version=1", "--no-deps", "--locked"], cwd=ROOT, timeout=60
    ))
    errors = dependency_errors(metadata["packages"])

    exceptions = json.loads((ROOT / "scripts/architecture-exceptions.json").read_text())
    files = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=ROOT
    ).decode().split("\0")
    encountered = set()
    for name in set(files):
        path = ROOT / name
        if not name.endswith(".rs") or not path.is_file() or not name.startswith(("apps/", "crates/", "tools/")):
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
