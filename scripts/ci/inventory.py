"""Validate the full execution inventory independently of shadow selection."""
from pathlib import Path
import shlex
import re

from common import ROOT

# This is a required-capability list, not a selectable job list. Deleting a job
# in the workflow must fail validation rather than silently shrink the baseline.
REQUIRED_JOBS = {
    "plan", "fmt-check", "clippy", "test", "test-doc", "openapi-diff",
    "rust-gate", "compatibility", "docs", "web-wasm", "build-release",
    "resource-soak", "supply-chain", "browser-e2e", "docker", "multiarch",
}


def workspace(metadata, root=ROOT):
    packages = {p["name"]: p for p in metadata["packages"] if p["id"] in metadata["workspace_members"]}
    if not packages:
        raise ValueError("empty workspace inventory")
    owners, reverse = {}, {name: set() for name in packages}
    for name, package in packages.items():
        manifest = Path(package["manifest_path"]).resolve()
        owners[name] = manifest.parent.relative_to(root.resolve()).as_posix() + "/"
        for dependency in package["dependencies"]:
            dep = dependency["name"]
            if dependency.get("path"):
                if dep not in packages:
                    raise ValueError(f"unregistered local dependency: {name} -> {dep}")
                expected = Path(packages[dep]["manifest_path"]).parent.resolve()
                if Path(dependency["path"]).resolve() != expected:
                    raise ValueError(f"ambiguous dependency path: {name} -> {dep}")
                # Conservatively include every kind, target and optional edge.
                reverse[dep].add(name)
    return owners, reverse


def shards(workflow, packages):
    jobs = workflow["jobs"]
    if set(jobs) != REQUIRED_JOBS | {"acceptance-gate"}:
        raise ValueError("workflow jobs differ from the required full inventory")
    gate = jobs["acceptance-gate"]
    if set(gate["needs"]) != REQUIRED_JOBS or gate.get("if") != "always()":
        raise ValueError("final gate must always depend on every required job")
    test = jobs["test"]
    if "if" in test or test.get("continue-on-error", False):
        raise ValueError("full Rust matrix cannot skip or ignore failures")
    if test["strategy"]["fail-fast"] is not False:
        raise ValueError("all Rust shards must finish for complete feedback")
    if test["strategy"]["matrix"].keys() != {"include"}:
        raise ValueError("only the complete static include matrix is supported")
    mapped = matrix_shards(workflow)
    seen = {package for members in mapped.values() for package in members}
    if seen != set(packages):
        raise ValueError(f"shard coverage mismatch: missing={set(packages)-seen}, extra={seen-set(packages)}")
    return mapped


def matrix_shards(workflow):
    """Read the single static partition authority without needing a compiler."""
    test = workflow["jobs"]["test"]
    mapped, seen = {}, set()
    for entry in test["strategy"]["matrix"]["include"]:
        shard = entry["shard"]
        if not isinstance(shard, str) or not re.fullmatch(r"[a-z][a-z0-9_-]{0,63}", shard):
            raise ValueError("invalid shard name")
        tokens = shlex.split(entry["crates"])
        if not tokens or len(tokens) % 2 or any(t != "-p" for t in tokens[::2]):
            raise ValueError(f"invalid package list for {shard}")
        members = tokens[1::2]
        if shard in mapped or len(members) != len(set(members)) or seen.intersection(members):
            raise ValueError(f"duplicate shard/package owner: {shard}")
        mapped[shard] = members
        seen.update(members)
    if not mapped:
        raise ValueError("empty shard matrix")
    return mapped


def owner(path, owners):
    matches = [name for name, prefix in owners.items() if path.startswith(prefix)]
    if len(matches) > 1:
        raise ValueError(f"ambiguous source owner: {path}")
    return matches[0] if matches else None


def cases(matrix, owners, root=ROOT):
    registered = {}
    for case in matrix["cases"]:
        cid, evidence = case["id"], case["evidence"]
        if cid in registered:
            raise ValueError(f"duplicate case: {cid}")
        status = evidence["status"]
        if status not in {"pass", "fail", "planned", "not-run", "blocked"}:
            raise ValueError(f"invalid historical status: {cid}")
        bindings = set()
        for ref in evidence.get("tests", []):
            # Legacy references include prose; use only the file association.
            # Never interpolate a registry reference into an executable command.
            path = ref.rsplit("::", 1)[0].split(" (", 1)[0]
            resolved = (root / path).resolve()
            if not resolved.is_relative_to(root.resolve()) or not resolved.is_file():
                raise ValueError(f"missing/unsafe case reference: {cid}: {path}")
            package = owner(path, owners)
            if package:
                bindings.add(package)
            elif path.startswith("tests/e2e/"):
                bindings.add("browser-e2e")
            else:
                raise ValueError(f"unregistered case owner: {cid}: {path}")
        if status == "pass" and not bindings:
            raise ValueError(f"pass case has no proof owner: {cid}")
        registered[cid] = {"historical_status": status, "owners": sorted(bindings), "execution_status": "not-run"}
    if not registered:
        raise ValueError("empty case inventory")
    return registered
