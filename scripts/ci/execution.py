"""Rust command receipts; the workflow matrix remains the partition authority."""
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import shlex

PREFIX = ["cargo", "test", "--locked", "--all-targets", "--all-features"]
RECEIPT_FIELDS = {
    "schema_version", "kind", "commit", "source_digest", "dirty", "toolchain",
    "lockfile_digest", "run_id", "run_attempt", "shard", "packages", "command",
    "profile", "target", "compiler", "compiler_host", "started_at",
    "finished_at", "elapsed_ms", "exit_code", "status", "diagnostic",
}


def package_args(value):
    tokens = shlex.split(value)
    if not tokens or len(tokens) % 2 or any(t != "-p" for t in tokens[::2]):
        raise ValueError("expected a nonempty sequence of -p PACKAGE pairs")
    packages = tokens[1::2]
    if len(packages) != len(set(packages)) or any(
        not re.fullmatch(r"deve-sub-[a-z0-9-]+", p) for p in packages
    ):
        raise ValueError("duplicate or invalid package name")
    return packages


def command(packages):
    return PREFIX + [arg for package in packages for arg in ("-p", package)]


def run_context(source, environ=None):
    environ = os.environ if environ is None else environ
    rid, attempt = environ.get("GITHUB_RUN_ID"), environ.get("GITHUB_RUN_ATTEMPT")
    if bool(rid) != bool(attempt):
        raise ValueError("incomplete workflow run identity")
    if environ.get("GITHUB_SHA", source["commit"]) != source["commit"]:
        raise ValueError("checkout differs from the workflow commit")
    return {"run_id": rid or "local", "run_attempt": attempt or "local"}


def scope(shard, packages, source, run):
    if not re.fullmatch(r"[a-z][a-z0-9_-]{0,63}", shard):
        raise ValueError("invalid shard name")
    return {
        "schema_version": 1, "kind": "rust-shard-execution", **source, **run,
        "shard": shard, "packages": packages, "command": command(packages),
        "profile": "test", "target": "cargo-default",
    }


def timestamp():
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds")


def compiler_fields(output, toolchain):
    lines = output.splitlines()
    hosts = [line.removeprefix("host: ") for line in lines if line.startswith("host: ")]
    if not lines or not lines[0].startswith(f"rustc {toolchain} ") or len(hosts) != 1:
        raise ValueError("compiler does not match the pinned toolchain")
    if hosts[0] != "x86_64-unknown-linux-gnu":
        raise ValueError("Rust shard receipts currently require a Linux amd64 host")
    return {"compiler": lines[0], "compiler_host": hosts[0]}


def validate(receipt, shard, packages, source, run):
    if not isinstance(receipt, dict) or set(receipt) != RECEIPT_FIELDS:
        raise ValueError("unknown or incomplete receipt schema")
    for key, value in scope(shard, packages, source, run).items():
        # JSON comparison distinguishes booleans from integers, unlike ==.
        if json.dumps(receipt[key], allow_nan=False) != json.dumps(value, allow_nan=False):
            raise ValueError(f"receipt scope mismatch: {key}")
    if receipt["status"] != "pass" or type(receipt["exit_code"]) is not int or receipt["exit_code"] != 0:
        raise ValueError(f"non-passing execution: {receipt['status']}")
    if receipt["diagnostic"] is not None:
        raise ValueError("passing receipt carries a failure diagnostic")
    if type(receipt["elapsed_ms"]) is not int or receipt["elapsed_ms"] < 0:
        raise ValueError("invalid elapsed time")
    times = []
    for field in ("started_at", "finished_at"):
        if not isinstance(receipt[field], str):
            raise ValueError(f"missing timestamp: {field}")
        parsed = datetime.fromisoformat(receipt[field])
        if parsed.utcoffset() != timezone.utc.utcoffset(parsed):
            raise ValueError(f"timestamp is not UTC: {field}")
        times.append(parsed)
    if times[1] < times[0]:
        raise ValueError("execution finished before it started")
    if not isinstance(receipt["compiler"], str) or "\n" in receipt["compiler"]:
        raise ValueError("invalid compiler version")
    if receipt["compiler_host"] != "x86_64-unknown-linux-gnu":
        raise ValueError("invalid compiler host")
    compiler_fields(f"{receipt['compiler']}\nhost: {receipt['compiler_host']}", source["toolchain"])


def strict_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON field: {key}")
        result[key] = value
    return result


def reject_constant(value):
    raise ValueError(f"invalid JSON constant: {value}")


def verify_collection(root, partitions, source, run):
    """Require each named artifact separately; merging can hide duplicates."""
    root = Path(root)
    results, errors = {}, []
    if root.is_symlink() or not root.is_dir():
        return results, ["missing or symlinked Rust receipt collection"]
    expected = {f"rust-execution-{name}" for name in partitions}
    actual = {path.name for path in root.iterdir()}
    if actual != expected:
        errors.append(f"receipt inventory mismatch: missing={sorted(expected-actual)}, extra={sorted(actual-expected)}")
    for shard, packages in sorted(partitions.items()):
        directory = root / f"rust-execution-{shard}"
        path = directory / "receipt.json"
        try:
            if directory.is_symlink() or not directory.is_dir():
                raise ValueError("missing or symlinked shard directory")
            if {p.name for p in directory.iterdir()} != {"receipt.json"}:
                raise ValueError("shard artifact must contain only receipt.json")
            if path.is_symlink() or not path.is_file() or path.stat().st_size > 65536:
                raise ValueError("missing, symlinked or oversized receipt")
            receipt = json.loads(path.read_text(), object_pairs_hook=strict_object, parse_constant=reject_constant)
            validate(receipt, shard, packages, source, run)
            results[shard] = {"status": "pass", "packages": packages,
                              "elapsed_ms": receipt["elapsed_ms"], "exit_code": 0}
        except (ValueError, TypeError, OSError) as error:
            results[shard] = {"status": "fail"}
            errors.append(f"{shard}: {error}")
    return results, errors
