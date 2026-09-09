#!/usr/bin/env python3
"""Real serve-process resource soak. Linux /proc, stdlib only, synthetic data.

Default 30 minutes; CI uses --seconds 90. Source refresh deliberately exercises
SSRF rejection against loopback; successful fetch/reconcile has separate adapter
and reconciliation tests. No production SSRF bypass is added for this harness.
"""
import argparse
import http.client
import json
import os
from pathlib import Path
import re
import signal
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time

SPEC = """apiVersion: deve-sub.io/v1
kind: SubscriptionTemplate
metadata:
  name: soak
  description: synthetic resource soak
  version: 1
spec:
  targetProfiles: [mihomo]
  variables: {}
  nodeSelector: {mode: dynamic}
  proxyGroups: []
  rules: []
  dns: {}
  tun: {}
  output: {}
"""


class Api:
    def __init__(self, port):
        self.port = port
        self.cookie = None
        self.requests = 0
        self.failures = 0
        self.statuses = {}

    def call(self, method, path, body=None, expected=(200,)):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=15)
        headers = {"Content-Type": "application/json", "Origin": f"http://127.0.0.1:{self.port}"}
        if self.cookie:
            headers["Cookie"] = self.cookie
        self.requests += 1
        try:
            conn.request(method, path, json.dumps(body) if body is not None else None, headers)
            response = conn.getresponse()
            self.statuses[response.status] = self.statuses.get(response.status, 0) + 1
            payload = response.read()
            assert response.status in expected, f"{method} request returned {response.status}"
            if response.getheader("Set-Cookie"):
                self.cookie = response.getheader("Set-Cookie").split(";")[0]
            return json.loads(payload) if response.getheader("Content-Type", "").startswith("application/json") else payload
        except Exception:
            self.failures += 1
            raise
        finally:
            conn.close()


def resources(pid, database, elapsed, cycles):
    status = Path(f"/proc/{pid}/status").read_text()
    return {
        "seconds": round(elapsed, 3), "cycles": cycles,
        "rss_bytes": int(re.search(r"VmRSS:\s+(\d+)", status)[1]) * 1024,
        "fds": len(list(Path(f"/proc/{pid}/fd").iterdir())),
        "database_bytes": database.stat().st_size,
        "wal_bytes": Path(str(database) + "-wal").stat().st_size if Path(str(database) + "-wal").exists() else 0,
    }


def summary(samples, field):
    values = [s[field] for s in samples]
    tail = samples[len(samples) // 2:]
    xs = [s["cycles"] for s in tail]
    ys = [s[field] for s in tail]
    xm, ym = sum(xs) / len(xs), sum(ys) / len(ys)
    denominator = sum((x - xm) ** 2 for x in xs)
    slope = sum((x - xm) * (y - ym) for x, y in zip(xs, ys)) / denominator if denominator else 0
    return {"initial": values[0], "peak": max(values), "final": values[-1],
            "tail_min": min(ys), "tail_max": max(ys), "tail_slope_per_cycle": round(slope, 3)}


def wait_job(api, path, nested=False):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        value = api.call("GET", path)
        state = value["run"]["status"] if nested else value["status"]
        if state in ("completed", "failed", "cancelled"):
            return state
        time.sleep(0.02)
    raise AssertionError("background job failed to reach terminal status")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--seconds", type=float, default=1800)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--require-telemetry", action="store_true")
    args = parser.parse_args()
    assert args.seconds >= 10, "at least ten seconds of work required"
    assert Path("/proc/self/status").exists(), "Linux /proc required"
    binary = args.binary.resolve()
    with tempfile.TemporaryDirectory(prefix="deve-sub-soak-") as directory:
        root = Path(directory)
        database = root / "app.db"
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        config = {"server": {"bind": f"127.0.0.1:{port}", "serve_web": False},
                  "database": {"path": str(database)},
                  "security": {"master_key_path": str(root / "master.key"),
                               "allow_master_key_generation": True, "cookie_secure": False}}
        config_path = root / "config.json"
        config_path.write_text(json.dumps(config))
        subprocess.run([str(binary), "migrate", "--db-path", str(database)], check=True, capture_output=True, timeout=30)
        log_path = root / "serve.log"
        with log_path.open("w") as log:
            child = subprocess.Popen([str(binary), "serve", "--config", str(config_path)], stdout=log, stderr=log,
                                     env={**os.environ, "RUST_LOG": "info", "NO_COLOR": "1"})
            try:
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline:
                    if child.poll() is not None:
                        raise AssertionError("serve exited during startup")
                    try:
                        with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                            break
                    except OSError:
                        time.sleep(0.05)
                else:
                    raise AssertionError("serve did not listen")
                api = Api(port)
                api.call("GET", "/health/ready")
                credentials = {"username": "soak-admin", "password": "TEST_SOAK_PASSWORD_123"}
                api.call("POST", "/api/v1/auth/setup", credentials, (201,))
                api.call("POST", "/api/v1/auth/login", credentials)
                template = api.call("POST", "/api/v1/templates", {"name": "soak", "description": "test", "spec_yaml": SPEC}, (201,))["template"]["id"]
                subscription = api.call("POST", "/api/v1/subscriptions", {
                    "name": "soak", "slug": "soak", "template_id": template,
                    "profile": "mihomo", "node_selection": {"mode": "dynamic"}}, (201,))
                sub_id = subscription["subscription"]["id"]
                token = subscription["token_plaintext"]
                source = api.call("POST", "/api/v1/sources", {
                    "name": "ssrf-failure-fixture", "source_type": "uri_list",
                    "url": "http://127.0.0.1/fixture", "auto_update": False}, (201,))["source"]["id"]
                # A local TCP peer provides successful probes without external I/O.
                listener = socket.socket()
                listener.bind(("127.0.0.1", 0))
                listener.listen()
                listener.settimeout(0.2)
                stop = threading.Event()
                def accept_loop():
                    while not stop.is_set():
                        try:
                            peer, _ = listener.accept()
                            peer.close()
                        except socket.timeout:
                            pass
                peer_thread = threading.Thread(target=accept_loop)
                peer_thread.start()
                try:
                    content = "\n".join(f"trojan://TEST_PASSWORD_{i}@127.0.0.1:{listener.getsockname()[1]}#Soak-{i}" for i in range(20))
                    import_body = {"source_type": "uri_list", "content": content}
                    api.call("POST", "/api/v1/nodes/import", import_body)
                    node_ids = [n["id"] for n in api.call("GET", "/api/v1/nodes?limit=100")["nodes"]]
                    start = time.monotonic()
                    cycles = 0
                    samples = [resources(child.pid, database, 0, cycles)]
                    next_sample = start + 2
                    while time.monotonic() - start < args.seconds:
                        api.call("GET", "/api/v1/nodes?limit=100")
                        api.call("GET", f"/sub/{token}/mihomo")
                        api.call("GET", f"/api/v1/subscriptions/{sub_id}/traffic")
                        api.call("POST", f"/api/v1/subscriptions/{sub_id}/traffic-correction", {"upload": 1, "download": 2, "note": "synthetic soak"}, (201,))
                        if cycles % 10 == 0:
                            api.call("POST", "/api/v1/nodes/import", import_body)
                            job = api.call("POST", f"/api/v1/sources/{source}/refresh", expected=(202,))
                            assert wait_job(api, f"/api/v1/sources/refresh-jobs/{job['job_id']}") == "failed"
                            run = api.call("POST", "/api/v1/probe-runs", {"probe_type": "tcp_connect", "node_ids": node_ids}, (201,))["run"]["id"]
                            assert wait_job(api, f"/api/v1/probe-runs/{run}", True) == "completed"
                            api.call("POST", "/api/v1/auth/login", {"username": f"absent-{cycles}", "password": "TEST_INVALID_PASSWORD"}, (401, 429))
                        cycles += 1
                        now = time.monotonic()
                        if now >= next_sample:
                            samples.append(resources(child.pid, database, now - start, cycles))
                            next_sample = now + 2
                        time.sleep(0.02)
                    samples.append(resources(child.pid, database, time.monotonic() - start, cycles))
                finally:
                    stop.set()
                    peer_thread.join(timeout=2)
                    listener.close()
                assert cycles >= 20, "insufficient representative work"
                # Read-only state invariants complement filesystem trend observations.
                with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as db:
                    unfinished = db.execute("SELECT count(*) FROM probe_runs WHERE status IN ('P','R')").fetchone()[0]
                    assert unfinished == 0
                    history = {table: db.execute(f"SELECT count(*) FROM {table}").fetchone()[0]
                               for table in ("subscription_traffic", "source_refresh_jobs", "probe_runs", "generation_cache", "sessions")}
                child.send_signal(signal.SIGTERM)
                child.wait(timeout=45)
                assert child.returncode == 0, "graceful shutdown failed"
                text = re.sub(r"\x1b\[[0-9;]*m", "", log_path.read_text())
                telemetry = {}
                for key in ("tracked_jobs", "task_panics", "task_cancellations", "rate_limiter_entries"):
                    values = [int(v) for v in re.findall(rf"\b{key}=(\d+)", text)]
                    telemetry[key] = {"initial": values[0], "peak": max(values), "final": values[-1]} if values else None
                if args.require_telemetry:
                    assert all(telemetry.values()), "missing runtime telemetry"
                    assert telemetry["tracked_jobs"]["final"] == 0
                    assert telemetry["tracked_jobs"]["peak"] <= 64
                    assert telemetry["rate_limiter_entries"]["peak"] <= 10_000
                    assert telemetry["task_panics"]["final"] == 0
                report = {"mode": "soak" if args.seconds >= 90 else "smoke",
                          "checkpoint_samples": len(re.findall(r"sqlite checkpoint\b.*\bbusy=", text)),
                          "seconds": samples[-1]["seconds"], "cycles": cycles,
                          "requests": api.requests, "failures": api.failures,
                          "request_failure_rate": api.failures / api.requests,
                          "statuses": api.statuses, "error_logs": len(re.findall(r"\bERROR\b", text)),
                          "resources": {key: summary(samples, key) for key in ("rss_bytes", "fds", "database_bytes", "wal_bytes")},
                          "telemetry": telemetry, "history_rows": history, "samples": samples,
                          "source_refresh": "real application SSRF failure path; successful reconciliation measured separately"}
                rss, fds = report["resources"]["rss_bytes"], report["resources"]["fds"]
                assert rss["final"] <= rss["tail_min"] + max(32 * 1024 * 1024, rss["tail_min"] // 2), "RSS exceeds generous tail envelope"
                assert fds["tail_max"] <= fds["tail_min"] + 8, "FD count does not stabilize"
                if args.seconds >= 90 and args.require_telemetry:
                    assert report["checkpoint_samples"] >= 3, "periodic checkpoint did not run"
                    wal = report["resources"]["wal_bytes"]
                    assert wal["tail_max"] <= wal["tail_min"] + 16 * 1024 * 1024, "WAL exceeds normal unpinned envelope"
                assert report["error_logs"] == 0, "unexpected server error logs"
                if args.output:
                    args.output.parent.mkdir(parents=True, exist_ok=True)
                    args.output.write_text(json.dumps(report, indent=2) + "\n")
                print(json.dumps({k: v for k, v in report.items() if k != "samples"}, indent=2))
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=5)


if __name__ == "__main__":
    main()
