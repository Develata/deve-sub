"""Validate the full execution inventory and exact Rust command."""
from pathlib import Path
import shlex
import re

from common import ROOT

# This is a required-capability list, not a selectable job list. Deleting a job
# in the workflow must fail validation rather than silently shrink the baseline.
REQUIRED_JOBS = {
    "inventory", "fmt-check", "clippy", "test", "test-doc", "openapi-diff",
    "rust-gate", "compatibility", "docs", "web-wasm", "build-release",
    "resource-soak", "supply-chain", "browser-e2e", "docker", "multiarch",
}


def workspace(metadata, root=ROOT):
    packages = {p["name"]: p for p in metadata["packages"] if p["id"] in metadata["workspace_members"]}
    if not packages:
        raise ValueError("empty workspace inventory")
    return {name: Path(package["manifest_path"]).resolve().parent.relative_to(root.resolve()).as_posix() + "/"
            for name, package in packages.items()}


def shards(workflow, packages):
    if any(key in workflow for key in ("env", "defaults")):
        raise ValueError("workflow cannot override Rust command environment or working directory")
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
    commands = [step for step in test["steps"] if "run" in step]
    expected = "cargo test --locked --all-targets --all-features $CI_PACKAGES"
    if len(commands) != 1 or commands[0].get("run") != expected:
        raise ValueError("Rust shard must run the exact full Cargo command")
    step = commands[0]
    if set(step) != {"name", "env", "run"} or step["env"] != {"CI_PACKAGES": "${{ matrix.crates }}"}:
        raise ValueError("Rust test command cannot be filtered, skipped or error-suppressed")
    if any(key in test for key in ("env", "defaults")):
        raise ValueError("Rust test job cannot override command environment or working directory")
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



def main():
    import json
    import subprocess
    import yaml
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"], cwd=ROOT, timeout=60))
    packages = workspace(metadata)
    mapped = shards(yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text()), packages)
    print(f"Full static baseline: {len(mapped)} shards, {len(packages)} packages")


if __name__ == "__main__":
    main()
