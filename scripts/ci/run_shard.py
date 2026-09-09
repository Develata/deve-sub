#!/usr/bin/env python3
"""Run a complete Rust shard and preserve its actual command outcome."""
import argparse
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

sys.dont_write_bytecode = True
from common import ROOT, identity, write_json
from execution import compiler_fields, package_args, run_context, scope, timestamp


def stop_group(child):
    """Bound cleanup of the invocation's own Cargo/test process group."""
    try:
        os.killpg(child.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        pass
    # Cargo may exit before one of its test children. Stop the group as well.
    try:
        os.killpg(child.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    child.wait()


def execute(command, root):
    cancelled = None
    child = None

    def cancel(signum, _frame):
        nonlocal cancelled
        cancelled = signum

    previous = {sig: signal.signal(sig, cancel) for sig in (signal.SIGINT, signal.SIGTERM)}
    try:
        child = subprocess.Popen(command, cwd=root, start_new_session=True)
        while True:
            if cancelled is not None:
                stop_group(child)
                return 128 + cancelled, True
            try:
                code = child.wait(timeout=0.2)
                if cancelled is not None:
                    stop_group(child)
                    return 128 + cancelled, True
                return (code if code >= 0 else 128 - code), False
            except subprocess.TimeoutExpired:
                continue
    finally:
        if child is not None and child.poll() is None:
            stop_group(child)
        for sig, handler in previous.items():
            signal.signal(sig, handler)


def run(shard, packages, output, root=ROOT):
    source = identity(root)
    receipt = {
        **scope(shard, packages, source, run_context(source)),
        "compiler": None, "compiler_host": None, "started_at": timestamp(),
        "finished_at": None, "elapsed_ms": None, "exit_code": None,
        "status": "running", "diagnostic": None,
    }
    write_json(output, receipt)
    started = time.monotonic_ns()
    code = 1
    try:
        if os.environ.get("CARGO_BUILD_TARGET"):
            raise ValueError("explicit Cargo target override is outside the native shard contract")
        compiler = subprocess.check_output(["rustc", "--version", "--verbose"], cwd=root, text=True)
        receipt.update(compiler_fields(compiler, source["toolchain"]))
        write_json(output, receipt)
        code, cancelled = execute(receipt["command"], root)
        receipt.update({"exit_code": code, "status": "cancelled" if cancelled else "fail" if code else "pass"})
        if identity(root) != source:
            receipt.update({"status": "fail", "diagnostic": "source changed during shard execution"})
            code = code or 1
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        receipt.update({"status": "fail", "diagnostic": str(error).replace("\n", " ")[:256]})
        code = 1
    finally:
        receipt.update({"finished_at": timestamp(), "elapsed_ms": (time.monotonic_ns()-started)//1_000_000})
        write_json(output, receipt)
    print(f"Rust shard {shard}: {receipt['status']}, exit={receipt['exit_code']}, elapsed={receipt['elapsed_ms']}ms")
    return code


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shard", required=True)
    parser.add_argument("--packages", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    raise SystemExit(run(args.shard, package_args(args.packages), args.output))


if __name__ == "__main__":
    main()
