#!/usr/bin/env python3
"""Create or verify a source-bound manifest before consuming a CI artifact."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

sys.dont_write_bytecode = True
from common import digest, identity, write_json

MANIFEST = ".ci-artifact.json"


def files(root, kind):
    if not root.is_dir() or root.is_symlink():
        raise ValueError("artifact root must be a regular directory")
    result = {}
    for path in sorted(root.rglob("*")):
        name = path.relative_to(root).as_posix()
        if path.is_symlink():
            raise ValueError(f"artifact symlink forbidden: {name}")
        if path.is_dir():
            continue
        if not path.is_file():
            raise ValueError(f"artifact is not a regular file: {name}")
        if name != MANIFEST:
            result[name] = {"sha256": digest(path), "size": path.stat().st_size}
    if kind == "binary":
        if set(result) != {"deve-sub"} or not result["deve-sub"]["size"]:
            raise ValueError("binary artifact must contain exactly one nonempty deve-sub")
    elif kind == "wasm":
        if "index.html" not in result or not result["index.html"]["size"]:
            raise ValueError("WASM artifact missing index.html")
        for extension in (".wasm", ".js", ".css"):
            if not any(n.startswith("assets/") and n.endswith(extension) and v["size"] for n, v in result.items()):
                raise ValueError(f"WASM artifact missing nonempty {extension}")
    else:
        raise ValueError(f"unknown artifact kind: {kind}")
    return result


def context(kind, source):
    return {
        "schema_version": 1, "kind": kind, **source,
        "target": "wasm32-unknown-unknown" if kind == "wasm" else "x86_64-unknown-linux-gnu",
        "profile": "release", "features": ["frontend"] if kind == "wasm" else [],
        "run_id": os.environ.get("GITHUB_RUN_ID", "local"),
        "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT", "local"),
    }


def create(root, kind, source, compiler):
    expected = context(kind, source)
    if not compiler.startswith(f"rustc {source['toolchain']} "):
        raise ValueError("build compiler differs from the pinned toolchain")
    manifest = {**expected, "compiler": compiler, "files": files(root, kind)}
    write_json(root / MANIFEST, manifest)
    return manifest


def verify(root, kind, source, manifest_sha256):
    path = root / MANIFEST
    if path.is_symlink() or not path.is_file():
        raise ValueError("missing or symlinked artifact manifest")
    # The expected digest travels in the producer's job output, independently
    # of the downloaded payload. A replacement manifest cannot bless new bytes.
    if not re.fullmatch(r"[0-9a-f]{64}", manifest_sha256 or "") or digest(path) != manifest_sha256:
        raise ValueError("manifest digest differs from the producer job output")
    manifest = json.loads(path.read_text())
    expected = context(kind, source)
    if set(manifest) != set(expected) | {"compiler", "files"}:
        raise ValueError("unknown or incomplete manifest schema")
    for key, value in expected.items():
        if manifest[key] != value:
            raise ValueError(f"artifact context mismatch: {key}")
    if not isinstance(manifest["compiler"], str) or not manifest["compiler"].startswith(f"rustc {source['toolchain']} "):
        raise ValueError("artifact compiler differs from the pinned toolchain")
    if manifest["files"] != files(root, kind):
        raise ValueError("artifact file inventory, size or digest mismatch")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("create", "verify"))
    parser.add_argument("--kind", choices=("binary", "wasm"), required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--manifest-sha256", help="required on verify; producer job output")
    args = parser.parse_args()
    source = identity()
    if args.operation == "create":
        compiler = subprocess.check_output(["rustc", "--version"], text=True).strip()
        manifest = create(args.root, args.kind, source, compiler)
    else:
        manifest = verify(args.root, args.kind, source, args.manifest_sha256)
    print(f"artifact {args.operation}: {args.kind}, {manifest['commit']}, {len(manifest['files'])} verified files")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"artifact rejected: {error}") from error
