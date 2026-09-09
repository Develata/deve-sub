#!/usr/bin/env python3
"""Fail closed on the complete job inventory, independently of shadow plans."""
import argparse
import json
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from common import write_json
from inventory import REQUIRED_JOBS


def evaluate(needs, event):
    errors, results = [], {}
    if set(needs) != REQUIRED_JOBS:
        errors.append(f"job inventory mismatch: missing={sorted(REQUIRED_JOBS-set(needs))}, extra={sorted(set(needs)-REQUIRED_JOBS)}")
    for job in sorted(REQUIRED_JOBS):
        result = needs.get(job, {}).get("result", "missing")
        allowed_skip = job == "multiarch" and event == "pull_request" and result == "skipped"
        results[job] = {"status": "not-run" if allowed_skip else result}
        if allowed_skip:
            results[job]["reason"] = "multiarch is excluded on PR by the full baseline policy"
        elif result != "success":
            errors.append(f"{job}: {result}")
    return {"schema_version": 1, "profile": "full", "jobs": results,
            "status": "fail" if errors else "pass", "errors": errors,
            "case_execution": "not asserted by job aggregation"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = evaluate(json.loads(os.environ["CI_NEEDS"]), os.environ["GITHUB_EVENT_NAME"])
    report.update({"commit": os.environ["GITHUB_SHA"], "run_id": os.environ["GITHUB_RUN_ID"],
                   "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"]})
    write_json(args.output, report)
    print(json.dumps(report, indent=2))
    if report["errors"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
