#!/usr/bin/env python3
"""DEPLOY-003/004/005: verify an existing image's default runtime and healthcheck."""
import argparse
import json
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid

LABEL = "io.deve-sub.health-test"
HEALTHCHECK = ["CMD", "/app/deve-sub", "health", "live"]
HTTP_DEADLINE_SECONDS = 5


class SmokeFailure(RuntimeError):
    """A diagnostic known not to contain container metadata or response bodies."""


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def interrupted(signum, frame):
    if signum == signal.SIGALRM:
        raise SmokeFailure(f"HTTP probe exceeded the {HTTP_DEADLINE_SECONDS:g}-second deadline")
    raise KeyboardInterrupt


def probe(opener, url):
    # Socket timeouts alone allow indefinitely slow responses to keep arriving.
    previous = signal.signal(signal.SIGALRM, interrupted)
    signal.setitimer(signal.ITIMER_REAL, HTTP_DEADLINE_SECONDS)
    try:
        with opener.open(url, timeout=5) as response:
            return response.status, response.read(1024 * 1024)
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


def docker(*args, timeout=15, missing_ok=False):
    try:
        result = subprocess.run(["docker", *args], capture_output=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        raise SmokeFailure(f"docker {args[0]} timed out") from None
    except OSError:
        raise SmokeFailure("could not execute Docker CLI") from None
    if result.returncode:
        if missing_ok and (b"No such object" in result.stderr or b"No such container" in result.stderr):
            return None
        # Docker errors may contain environment values or registry credentials.
        raise SmokeFailure(f"docker {args[0]} failed (exit {result.returncode})")
    return result.stdout


def require(condition, message):
    if not condition:
        raise SmokeFailure(message)


def cleanup(name, identity):
    # Create can succeed in the daemon even if its CLI acknowledgement times out.
    raw = docker("container", "inspect", name, missing_ok=True)
    if raw is None:
        return
    container = json.loads(raw)[0]
    require((container["Config"].get("Labels") or {}).get(LABEL) == identity,
            "cleanup label mismatch; refusing removal")
    try:
        if container["State"]["Running"]:
            docker("stop", "--time", "5", name)
    finally:
        docker("container", "rm", "--force", name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True, help="Already present image; never pulled")
    parser.add_argument("--platform", required=True, choices=("linux/amd64", "linux/arm64"))
    args = parser.parse_args()
    metadata = json.loads(docker("image", "inspect", args.image))[0]
    actual_platform = f"{metadata['Os']}/{metadata['Architecture']}"
    require(actual_platform == args.platform, "image OS/architecture differs from requested platform")
    require((metadata["Config"].get("Healthcheck") or {}).get("Test") == HEALTHCHECK,
            "image must declare CMD /app/deve-sub health live")
    require(set(metadata["Config"].get("Volumes") or {}) <= {"/app/data"},
            "image declares an unexpected volume; refusing anonymous storage")
    # Pin the inspected image, so a concurrently moved tag cannot change the test.
    image_id = metadata["Id"]
    identity = uuid.uuid4().hex
    name = f"deve-sub-health-test-{identity}"
    try:
        docker("create", "--name", name, "--label", f"{LABEL}={identity}",
               "--pull", "never", "--platform", args.platform,
               "--publish", "127.0.0.1::8080", "--tmpfs",
               "/app/data:rw,noexec,nosuid,nodev,size=128m,uid=1000,gid=1000,mode=0700",
               image_id)
        started = time.monotonic()
        docker("start", name)
        deadline = started + 60
        while True:
            remaining = deadline - time.monotonic()
            require(remaining > 0, "Docker health did not become healthy within 60 seconds")
            container = json.loads(docker("container", "inspect", name,
                                          timeout=min(10, remaining)))[0]
            state = container["State"]
            require(state["Running"], "default entrypoint exited before becoming healthy")
            health = state.get("Health", {}).get("Status")
            if health == "healthy":
                health_seconds = time.monotonic() - started
                require(health_seconds <= 60, "Docker health exceeded the 60-second deadline")
                break
            require(health in ("starting", "unhealthy"), "container has no active Docker healthcheck")
            time.sleep(min(1, max(0, deadline - time.monotonic())))
        require(container["Config"]["Healthcheck"] == metadata["Config"]["Healthcheck"],
                "container healthcheck differs from image defaults")
        require(not any(mount["Type"] == "volume" for mount in container["Mounts"]),
                "container unexpectedly created anonymous storage")
        bindings = container["NetworkSettings"]["Ports"]["8080/tcp"]
        require(len(bindings) == 1 and bindings[0]["HostIp"] == "127.0.0.1",
                "container port is not exclusively bound to loopback")
        base = f"http://127.0.0.1:{int(bindings[0]['HostPort'])}"
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
        statuses = {}
        for path in ("/health/live", "/health/ready", "/"):
            try:
                statuses[path], body = probe(opener, base + path)
            except (OSError, urllib.error.URLError):
                raise SmokeFailure(f"HTTP probe failed for {path}") from None
            require(statuses[path] == 200, f"HTTP probe returned non-200 for {path}")
            if path == "/":
                require(b"deve-sub-web" in body, "root page does not contain the compiled Web application")
        require(docker("exec", name, "id", "-u").strip() == b"1000",
                "container runtime user is not UID 1000")
        evidence = {"cases": ["DEPLOY-003" if args.platform == "linux/amd64" else "DEPLOY-004",
                              "DEPLOY-005"], "status": "pass", "image_id": image_id,
                    "platform": actual_platform, "healthcheck": HEALTHCHECK,
                    "health_status": health, "health_seconds": round(health_seconds, 3),
                    "endpoints": statuses, "uid": 1000, "anonymous_volumes": 0}
    finally:
        # Finish bounded cleanup even if CI repeats its termination signal.
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        cleanup(name, identity)
    # A pass is emitted only after the owned container has been removed.
    print(json.dumps(evidence, sort_keys=True))


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, interrupted)
    try:
        main()
    except (Exception, KeyboardInterrupt) as error:
        # Schema/parser failures must not echo Docker metadata or response bodies.
        message = str(error) if isinstance(error, SmokeFailure) else "unexpected smoke failure"
        if isinstance(error, KeyboardInterrupt):
            message = "smoke interrupted"
        print(json.dumps({"status": "fail", "error": message}), file=sys.stderr)
        sys.exit(1)
