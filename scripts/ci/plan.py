#!/usr/bin/env python3
"""Shadow-only impact proposal. The actual CI execution profile is always full."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
import yaml
from common import ROOT, git, identity, write_json
from inventory import REQUIRED_JOBS, cases, owner, shards, workspace

FULL_PREFIXES = (
    ".github/", ".cargo/", "docs/", "scripts/", "migrations/", "tests/fixtures/",
    "tests/acceptance/", "crates/deve-sub-contract/", "crates/deve-sub-domain/",
    "crates/deve-sub-kernel/", "crates/deve-sub-security/",
    "crates/deve-sub-application/src/auth/", "apps/server/src/auth/",
    "apps/cli/src/backup", "apps/cli/src/restore", "apps/cli/src/update",
    "apps/cli/tests/backup", "apps/cli/tests/update",
)
FULL_NAMES = {"AGENTS.md", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "Dockerfile", ".dockerignore", "docker-compose.yml", "docker-entrypoint.sh", "deny.toml", "crates/deve-sub-application/src/config.rs"}
RUNTIME = {"compatibility", "web-wasm", "browser-e2e", "resource-soak", "docker", "multiarch"}


def changed_paths(base, root=ROOT):
    if not base:
        return [], "comparison base unavailable"
    try:
        # Disable rename coalescing so both removed and added ownership enter
        # the proposal; compare the merge base, not merely HEAD's first parent.
        merge_base = git("merge-base", base, "HEAD", root=root).decode().strip()
        committed = git("diff", "--name-only", "--no-renames", "-z", merge_base, "HEAD", root=root)
        dirty = git("diff", "HEAD", "--name-only", "--no-renames", "-z", root=root)
        untracked = git("ls-files", "--others", "--exclude-standard", "-z", root=root)
        return sorted({p.decode() for p in (committed + dirty + untracked).split(b"\0") if p}), None
    except (subprocess.CalledProcessError, UnicodeError):
        return [], "comparison base unreadable; full fallback"


def propose(paths, owners, reverse, partitions, registered, force=None):
    reasons, selected = [], set()
    full = bool(force)
    if force:
        reasons.append(force)
    for path in paths:
        package = owner(path, owners)
        if (path in FULL_NAMES or path.startswith(FULL_PREFIXES)
                or Path(path).name in {"Cargo.toml", "build.rs", "AGENTS.md"}
                or "/fixtures/" in path):
            full = True
            reasons.append(f"full boundary: {path}")
        elif package:
            selected.add(package)
            reasons.append(f"package owner: {path} -> {package}")
        elif path.startswith("tests/e2e/"):
            selected.update({"deve-sub-web", "deve-sub-cli"} & set(owners))
            reasons.append(f"browser runtime input: {path}")
        else:
            full = True
            reasons.append(f"unknown impact: {path}")
    queue = list(selected)
    while queue:
        dependency = queue.pop()
        for consumer in sorted(reverse[dependency] - selected):
            selected.add(consumer)
            queue.append(consumer)
            reasons.append(f"reverse consumer: {dependency} -> {consumer}")
    if full:
        selected = set(owners)
    runtime = set()
    if full or "deve-sub-cli" in selected:
        runtime |= RUNTIME
    if selected & {"deve-sub-protocol", "deve-sub-emitter", "deve-sub-compatibility"}:
        runtime.add("compatibility")
    if "deve-sub-web" in selected:
        runtime |= {"web-wasm", "browser-e2e"}
    case_ids = [cid for cid, case in registered.items() if full or set(case["owners"]) & (selected | runtime)]
    return {
        "profile": "full" if full else "affected",
        "packages": sorted(selected),
        "shards": sorted(s for s, members in partitions.items() if selected.intersection(members)),
        "runtime": sorted(runtime), "cases": sorted(case_ids), "reasons": reasons,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base")
    parser.add_argument("--event", default="local")
    parser.add_argument("--ref", default="")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"], cwd=ROOT))
    owners, reverse = workspace(metadata)
    workflow = yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text())
    partitions = shards(workflow, owners)
    registered = cases(yaml.safe_load((ROOT / "tests/acceptance/matrix.yaml").read_text()), owners)
    paths, fallback = changed_paths(args.base)
    if args.event != "local" and (args.event != "pull_request" or args.ref == "refs/heads/main"):
        fallback = f"mandatory full event/ref: {args.event} {args.ref}"
    report = {
        "schema_version": 1, **identity(), "selection_mode": "shadow",
        "execution_profile": "full", "comparison_base": args.base,
        "changed_paths": paths, "executed_expected": sorted(REQUIRED_JOBS),
        "would_select": propose(paths, owners, reverse, partitions, registered, fallback),
        "reverse_consumers": {n: sorted(c) for n, c in reverse.items()},
        "registered_cases": registered,
    }
    if args.output:
        write_json(args.output, report)
        print(f"shadow plan: full execution; proposed {len(report['would_select']['shards'])}/{len(partitions)} Rust shards; {len(registered)} registered cases; {args.output}")
    else:
        print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"CI plan failed: {error}") from error
