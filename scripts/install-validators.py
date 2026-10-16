#!/usr/bin/env python3
"""Install the reviewed compatibility binaries only after SHA-256 verification.

Hashes match publisher GitHub release asset digests reviewed on 2026-10-16.
Mihomo 1.19.30 adds AnyTLS/Snell v4 relative to the old baseline; sing-box
1.14.3 supplies Snell and the current WireGuard endpoint schema. Hash pinning
is not publisher signing.
"""
import argparse
import gzip
import hashlib
import shutil
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path

ASSETS = [
    ("mihomo", "https://github.com/MetaCubeX/mihomo/releases/download/v1.19.30/mihomo-linux-amd64-v1-v1.19.30.gz", "cbe553d0319a414bd3a372c5976a252155b2c4882b66bce88a4d6bba9571a553"),
    ("sing-box", "https://github.com/SagerNet/sing-box/releases/download/v1.14.3/sing-box-1.14.3-linux-amd64.tar.gz", "e2bdf179c15a3652955dc44867e33ca0965c9c9b91b34267dcc5a5d639a5feee"),
    ("xray", "https://github.com/XTLS/Xray-core/releases/download/v26.3.27/Xray-linux-64.zip", "23cd9af937744d97776ee35ecad4972cf4b2109d1e0fe6be9930467608f7c8ae"),
]


def install(dest):
    dest.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="deve-sub-validators-") as work:
        for name, url, expected in ASSETS:
            archive = Path(work) / url.rsplit("/", 1)[1]
            subprocess.run(["curl", "--fail", "--location", "--silent", "--show-error", "--connect-timeout", "10", "--max-time", "300", url, "--output", str(archive)], check=True)
            with archive.open("rb") as data:
                actual = hashlib.file_digest(data, "sha256").hexdigest()
            if actual != expected:
                raise SystemExit(f"{name}: SHA-256 mismatch; refusing extraction/execution")
            target = dest / name
            # Copy only the exact binary member; never extract archive paths.
            if name == "mihomo":
                with gzip.open(archive, "rb") as source, target.open("wb") as output:
                    shutil.copyfileobj(source, output)
            elif name == "sing-box":
                with tarfile.open(archive) as bundle:
                    member = bundle.getmember("sing-box-1.14.3-linux-amd64/sing-box")
                    if not member.isfile():
                        raise SystemExit("sing-box: binary member is not a regular file")
                    with bundle.extractfile(member) as source, target.open("wb") as output:
                        shutil.copyfileobj(source, output)
            else:
                with zipfile.ZipFile(archive) as bundle, bundle.open("xray") as source, target.open("wb") as output:
                    shutil.copyfileobj(source, output)
            target.chmod(0o755)
            subprocess.run([str(target), "-v" if name == "mihomo" else "version"], check=True, timeout=10)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_directory", type=Path)
    install(parser.parse_args().output_directory.resolve())
