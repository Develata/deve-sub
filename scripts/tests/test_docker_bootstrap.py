#!/usr/bin/env python3
"""AUTH-001/DEPLOY-001: exercise bootstrap through Compose, entrypoint and HTTP."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[2]
USERNAME = "-fixture-admin"
PASSWORD = "fixture-$bootstrap-password!"
CHANGED_PASSWORD = "changed-fixture-password!"


def run(args, *, env=None, check=True):
    result = subprocess.run(args, env=env, capture_output=True, timeout=90)
    if check and result.returncode:
        # Container output can contain credentials after a regression; do not
        # dump stdout/stderr from the consumer whose redaction is under test.
        raise AssertionError(f"command failed with exit {result.returncode}: {args[:3]}")
    return result


def request(base, path, data=None):
    req = urllib.request.Request(base + path,
        data=None if data is None else json.dumps(data).encode(),
        headers={"Content-Type": "application/json"})
    try:
        with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req, timeout=5) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


def scenario(image, configured=True, invalid=None):
    project = "deve-sub-bootstrap-" + uuid.uuid4().hex[:12]
    with tempfile.TemporaryDirectory(prefix="deve-sub-bootstrap-") as directory:
        root = Path(directory)
        source = (ROOT / "docker-compose.yml").read_text()
        # Use the actual Compose interpolation and entrypoint, changing only
        # the test image, host port and restart policy for bounded bad-input runs.
        source, image_replacements = re.subn(
            r"ghcr\.io/develata/deve-sub:\$\{DEVE_SUB_IMAGE_TAG:-v\d+\.\d+\.\d+\}",
            lambda _: image, source, count=1)
        assert image_replacements == 1, "Compose release image was not replaced"
        source = source.replace('"8080:8080"', '"127.0.0.1:0:8080"')
        source = source.replace("restart: unless-stopped", 'restart: "no"')
        compose_file = root / "docker-compose.yml"
        compose_file.write_text(source)
        env_file = root / ".env"
        if configured:
            username, password = USERNAME, PASSWORD
            if invalid == "missing-password":
                password = ""
            elif invalid == "missing-username":
                username = ""
            elif invalid == "short-password":
                password = "short"
            env_file.write_text(f"DEVE_SUB_ADMIN_USERNAME='{username}'\n"
                                f"DEVE_SUB_ADMIN_PASSWORD='{password}'\n")
            env_file.chmod(0o600)
        env = {k: v for k, v in os.environ.items() if k not in
               ("DEVE_SUB_ADMIN_USERNAME", "DEVE_SUB_ADMIN_PASSWORD", "DEVE_SUB_IMAGE_TAG")}
        compose = ["docker", "compose", "-p", project, "-f", str(compose_file)]
        try:
            run(compose + ["up", "-d", "--no-build", "--pull", "never"], env=env)
            cid = run(compose + ["ps", "-aq", "deve-sub"], env=env).stdout.decode().strip()
            assert cid
            deadline = time.monotonic() + 60
            while True:
                state = json.loads(run(["docker", "inspect", cid]).stdout)[0]["State"]
                if state["Status"] == "exited" or state.get("Health", {}).get("Status") == "healthy":
                    break
                assert time.monotonic() < deadline, "container startup deadline exceeded"
                time.sleep(0.5)
            if invalid:
                assert state["Status"] == "exited" and state["ExitCode"] != 0
                logs = run(["docker", "logs", cid])
                assert PASSWORD.encode() not in logs.stdout + logs.stderr
                print(f"PASS {invalid}: fail before HTTP startup", flush=True)
                return
            assert state.get("Health", {}).get("Status") == "healthy"
            address = run(compose + ["port", "deve-sub", "8080"], env=env).stdout.decode().strip()
            base = "http://" + address
            code, payload = request(base, "/api/v1/auth/status")
            assert code == 200 and json.loads(payload)["initialized"] == configured
            if not configured:
                code, _ = request(base, "/api/v1/auth/setup", {"username": USERNAME, "password": PASSWORD})
                assert code == 201
            code, _ = request(base, "/api/v1/auth/login", {"username": USERNAME, "password": PASSWORD})
            assert code == 200
            process_env = run(["docker", "exec", cid, "cat", "/proc/1/environ"]).stdout
            assert b"DEVE_SUB_ADMIN_PASSWORD=" not in process_env
            assert b"DEVE_SUB_ADMIN_USERNAME=" not in process_env
            logs = run(["docker", "logs", cid])
            assert PASSWORD.encode() not in logs.stdout + logs.stderr
            print(f"PASS {'environment' if configured else 'Web'} initialization and login; credentials not inherited/logged", flush=True)
            if configured:
                env_file.write_text("DEVE_SUB_ADMIN_USERNAME='replacement-admin'\n"
                                    f"DEVE_SUB_ADMIN_PASSWORD='{CHANGED_PASSWORD}'\n")
                run(compose + ["up", "-d", "--no-build", "--pull", "never", "--force-recreate",
                               "--wait", "--wait-timeout", "60"], env=env)
                address = run(compose + ["port", "deve-sub", "8080"], env=env).stdout.decode().strip()
                base = "http://" + address
                assert request(base, "/api/v1/auth/login", {"username": USERNAME, "password": PASSWORD})[0] == 200
                assert request(base, "/api/v1/auth/login", {"username": USERNAME, "password": CHANGED_PASSWORD})[0] == 401
                assert request(base, "/api/v1/auth/login", {"username": "replacement-admin", "password": CHANGED_PASSWORD})[0] == 401
                logs = run(compose + ["logs", "--no-color"], env=env)
                assert all(p.encode() not in logs.stdout + logs.stderr for p in [PASSWORD, CHANGED_PASSWORD])
                print("PASS recreation preserves original administrator/password", flush=True)
        finally:
            # All resources use the UUID project created exclusively by this run.
            run(compose + ["down", "--volumes", "--timeout", "10"], env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True, help="locally built image containing this entrypoint and CLI")
    args = parser.parse_args()
    scenario(args.image)
    scenario(args.image, configured=False)
    for invalid in ["missing-password", "missing-username", "short-password"]:
        scenario(args.image, invalid=invalid)


if __name__ == "__main__":
    main()
