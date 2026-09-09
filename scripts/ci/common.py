"""Source identity shared by CI planning and artifact handoff."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def git(*args, root=ROOT):
    return subprocess.check_output(["git", *args], cwd=root)


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def identity(root=ROOT):
    """Include dirty and untracked sources; a HEAD label alone is insufficient."""
    names = set(git("ls-files", "-z", "--cached", "--others", "--exclude-standard", root=root).split(b"\0"))
    hasher = hashlib.sha256()
    for name in sorted(names - {b""}):
        path = root / os.fsdecode(name)
        if path.is_symlink():
            payload = b"link:" + os.fsencode(os.readlink(path))
        elif path.is_file():
            payload = digest(path).encode()
        elif not path.exists():
            payload = b"deleted"
        else:
            raise ValueError(f"unsupported source entry: {os.fsdecode(name)}")
        executable = b"x" if path.is_file() and path.stat().st_mode & 0o111 else b"-"
        hasher.update(name + b"\0" + executable + payload + b"\0")
    toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    return {
        "commit": git("rev-parse", "HEAD", root=root).decode().strip(),
        "source_digest": hasher.hexdigest(),
        "dirty": bool(git("status", "--porcelain", "--untracked-files=all", root=root)),
        "toolchain": toolchain,
        "lockfile_digest": digest(root / "Cargo.lock"),
    }


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
