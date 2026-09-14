#!/usr/bin/env python3
"""LOG-001: exercise Compose rotation on a labeled, isolated disposable container."""
import argparse
import json
from pathlib import Path
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[2]


def docker(*args, timeout=30):
    return subprocess.run(["docker", *args], cwd=ROOT, check=True, capture_output=True,
                          timeout=timeout).stdout


def cleanup(name, label, identity):
    # A timed-out create may still have succeeded in the daemon. Inspect the
    # unique name even when create did not return a successful acknowledgement.
    inspected = subprocess.run(["docker", "inspect", name], capture_output=True, timeout=30)
    if inspected.returncode:
        if b"No such object" in inspected.stderr or b"No such container" in inspected.stderr:
            return
        inspected.check_returncode()
    current = json.loads(inspected.stdout)[0]
    if current["Config"]["Labels"].get(label) != identity:
        raise RuntimeError("cleanup label mismatch; refusing removal")
    if current["State"]["Running"]:
        docker("stop", "--time", "3", name, timeout=10)
    docker("rm", name)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--image", required=True, help="Already present image with /bin/sh; never pulled")
    args = parser.parse_args()
    config = json.loads(docker("compose", "config", "--format", "json"))
    logging = config["services"]["deve-sub"]["logging"]
    assert logging == {"driver": "local", "options": {"max-size": "10m", "max-file": "3", "compress": "true"}}
    image = docker("image", "inspect", args.image, "--format", "{{.Id}}").decode().strip()
    metadata = json.loads(docker("image", "inspect", image))[0]
    # Images may declare VOLUME (the app declares /app/data). Replace every
    # declared mount with tmpfs so Docker creates no orphaned anonymous volumes.
    memory_mounts = []
    for path in (metadata["Config"].get("Volumes") or {}):
        memory_mounts.extend(["--tmpfs", f"{path}:rw,noexec,nosuid,size=1m"])
    identity = uuid.uuid4().hex
    name = f"deve-sub-log-test-{identity}"
    label = "io.deve-sub.logging-test"
    try:
        # Exact production rotation settings; no port, disk mount, network or user data.
        docker("create", "--name", name, "--label", f"{label}={identity}",
               "--network", "none", "--memory", "64m", "--cpus", "1", "--pull", "never",
               "--log-driver", logging["driver"], "--log-opt", "max-size=10m",
               "--log-opt", "max-file=3", "--log-opt", "compress=true", "--entrypoint", "/bin/sh", *memory_mounts, image,
               "-c", 'line=$(printf "%01016d" 0); i=0; while [ "$i" -lt 45000 ]; do printf "%06d%s\\n" "$i" "$line"; i=$((i+1)); done; printf "fixture-end\\n"')
        container = json.loads(docker("inspect", name))[0]
        assert not any(mount["Type"] == "volume" for mount in container["Mounts"])
        docker("start", name)
        assert docker("wait", name, timeout=60).strip() == b"0"
        retained = docker("logs", name)
        lines = retained.splitlines()
        assert lines[-1] == b"fixture-end", "newest output must survive"
        assert int(lines[0][:6]) > 0, "oldest output must have rotated away"
        assert len(retained) < 31 * 1024 * 1024, "retained output must fit configured capacity"
        print(json.dumps({"case": "LOG-001", "status": "pass", "rotation": logging,
                          "written_records": 45000, "retained_bytes": len(retained),
                          "first_retained_record": int(lines[0][:6]), "end_preserved": True, "anonymous_volumes": 0}))
    finally:
        cleanup(name, label, identity)



if __name__ == "__main__":
    main()
