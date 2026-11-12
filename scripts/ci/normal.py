#!/usr/bin/env python3
"""Run one isolated normal lane or reject incomplete/stale aggregate results."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

sys.dont_write_bytecode = True
from common import ROOT, digest, write_json
from normal_results import catalogue, context, dns_results, evaluate, exact, playwright_results, specs

E2E = ROOT / "tests/e2e"
PLAYWRIGHT = ["node", "node_modules/@playwright/test/cli.js", "test", "--config", "normal.config.ts"]


def discovery(args=()):
    result = subprocess.run(
        [*PLAYWRIGHT, *args, "--list", "--reporter=json"], cwd=E2E,
        capture_output=True, text=True, timeout=60, check=True,
    )
    report = json.loads(result.stdout)
    if report.get("errors"):
        raise ValueError("normal test discovery failed")
    return [row[0] for row in specs(report)]


def inventory(plan):
    required = [case for lane in plan["lanes"].values()
                if lane["kind"] == "playwright" for case in lane["cases"]]
    exact(discovery(), required, "full discovered catalogue")


def execute(command, cwd, environment, log):
    """Cancel/reap this process group only; never leave its isolated server alive."""
    process = None
    try:
        with log.open("w") as output:
            process = subprocess.Popen(command, cwd=cwd, env=environment, stdout=output,
                                       stderr=subprocess.STDOUT, start_new_session=True)
            return process.wait(timeout=900)
    finally:
        if process and process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)


def run_lane(name, output):
    plan = catalogue()
    lane = plan["lanes"][name]
    output.mkdir(parents=True, exist_ok=True)
    report = {
        "schema_version": 1, "lane": name, "context": context(),
        "started_at": datetime.now(timezone.utc).isoformat(), "status": "FAIL",
        "exit_code": None, "errors": [],
        "cases": [{"id": case, "status": "NOT_RUN"} for case in lane["cases"]],
    }
    try:
        inventory(plan)
        env = os.environ.copy()
        env["DEVE_SUB_NORMAL_OUTPUT"] = str(output / "browser")
        binary = Path(env.get("DEVE_SUB_BINARY", ROOT / "target/release/deve-sub")).resolve()
        web = Path(env.get("DEVE_SUB_WEB_DIST", ROOT / "apps/web/dist")).resolve()
        env.update(DEVE_SUB_BINARY=str(binary), DEVE_SUB_WEB_DIST=str(web))
        report["binary_sha256"] = digest(binary)
        if lane["kind"] == "playwright":
            exact(discovery(lane["args"]), lane["cases"], f"{name} discovered shard")
            command, cwd = [*PLAYWRIGHT, *lane["args"]], E2E
        else:
            command, cwd = [sys.executable, str(ROOT / "scripts/acceptance/forward-proxy-dns.py"),
                            "--binary", str(binary), "--output", str(output / "dns.json")], ROOT
        report["command"] = command
        code = execute(command, cwd, env, output / "command.log")
        report["exit_code"] = code
        if lane["kind"] == "playwright":
            raw = output / "browser/results.json"
            report["cases"] = playwright_results(json.loads(raw.read_text()), lane["cases"])
        else:
            raw = output / "dns.json"
            report["cases"] = dns_results(json.loads(raw.read_text()), report["binary_sha256"], lane["cases"][0])
        report["raw_report_sha256"] = digest(raw)
        if code != 0 or any(case["status"] != "PASS" for case in report["cases"]):
            raise ValueError("nonzero execution or unsuccessful case")
        if context() != report["context"]:
            raise ValueError("source changed during normal execution")
        report["status"] = "PASS"
    except (ValueError, KeyError, TypeError, AttributeError, OSError, subprocess.SubprocessError, InterruptedError) as error:
        report["errors"].append(f"{type(error).__name__}: {error}")
    finally:
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        write_json(output / "summary.json", report)
    print(f"normal {name}: {report['status']}; {len(report['cases'])} required cases")
    return 0 if report["status"] == "PASS" else 1


def aggregate(directory, output):
    reports, errors = [], []
    plan = catalogue()
    for path in sorted(directory.rglob("summary.json")):
        try:
            report = json.loads(path.read_text())
            if not isinstance(report, dict) or not isinstance(report.get("lane"), str):
                raise ValueError("lane summary must be an object with a string lane")
            reports.append(report)
            lane = plan["lanes"].get(report.get("lane"))
            if lane:
                raw = path.parent / ("browser/results.json" if lane["kind"] == "playwright" else "dns.json")
                if not (path.parent / "command.log").is_file() or digest(raw) != report.get("raw_report_sha256"):
                    raise ValueError("missing command log or changed/missing raw result")
                if lane["kind"] == "playwright":
                    actual = playwright_results(json.loads(raw.read_text()), lane["cases"])
                else:
                    actual = dns_results(json.loads(raw.read_text()), report["binary_sha256"], lane["cases"][0])
                if actual != report.get("cases"):
                    raise ValueError("summary cases differ from raw execution result")
        except (ValueError, KeyError, TypeError, AttributeError, OSError) as error:
            errors.append(f"{path.name}: {error}")
    try:
        needs = json.loads(os.environ.get("CI_NORMAL_NEEDS", "{}"))
        if not isinstance(needs, dict):
            raise ValueError("job outcomes must be an object")
    except ValueError as error:
        needs = {}
        errors.append(str(error))
    result = evaluate(reports, plan, context(), needs)
    result["errors"].extend(errors)
    if errors:
        result["status"] = "FAIL"
    write_json(output, result)
    print(f"normal aggregate: {result['status']}; {len(result['errors'])} errors")
    return 0 if result["status"] == "PASS" else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("inventory", "run", "gate"))
    parser.add_argument("--lane", choices=tuple(catalogue()["lanes"]))
    parser.add_argument("--output", type=Path)
    parser.add_argument("--reports", type=Path)
    args = parser.parse_args()
    if args.operation == "inventory":
        inventory(catalogue())
        print("normal catalogue: all discovered cases have exactly one owner")
        return 0
    if not args.output or (args.operation == "run" and not args.lane) or (args.operation == "gate" and not args.reports):
        parser.error("run needs --lane/--output; gate needs --reports/--output")
    if args.operation == "run":
        return run_lane(args.lane, args.output.resolve())
    return aggregate(args.reports.resolve(), args.output.resolve())


if __name__ == "__main__":
    def interrupted(_number, _frame):
        raise InterruptedError("normal lane interrupted")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    raise SystemExit(main())
