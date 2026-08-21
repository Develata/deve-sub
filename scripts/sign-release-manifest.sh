#!/usr/bin/env bash
# Sign the release manifest with the Ed25519 release key (P0-04).
#
# Generates `deve-sub-manifest.json` (version, target, per-asset sha256 +
# size) and `deve-sub-manifest.json.sig` (raw 64-byte Ed25519 signature
# over the manifest JSON bytes).
#
# The signing key seed is read from the DEVE_SUB_RELEASE_KEY_SEED
# environment variable (hex-encoded, 64 hex chars = 32 bytes). It must
# correspond to the RELEASE_PUBLIC_KEY embedded in
# apps/cli/src/update_manifest.rs. NEVER commit the seed.
#
# Usage:
#   DEVE_SUB_RELEASE_KEY_SEED=<hex> \
#     scripts/sign-release-manifest.sh <release_dir> <version> <target_triple>
#
# Example:
#   DEVE_SUB_RELEASE_KEY_SEED=83b8... \
#     scripts/sign-release-manifest.sh release 0.2.0 x86_64-unknown-linux-gnu

set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "Usage: $0 <release_dir> <version> <target_triple>" >&2
    exit 1
fi

RELEASE_DIR="$1"
VERSION="$2"
TARGET="$3"

if [ -z "${DEVE_SUB_RELEASE_KEY_SEED:-}" ]; then
    echo "ERROR: DEVE_SUB_RELEASE_KEY_SEED is not set" >&2
    exit 1
fi

# Verify the seed is 64 hex chars (32 bytes).
if ! echo "$DEVE_SUB_RELEASE_KEY_SEED" | grep -qE '^[0-9a-fA-F]{64}$'; then
    echo "ERROR: DEVE_SUB_RELEASE_KEY_SEED must be 64 hex chars (32 bytes)" >&2
    exit 1
fi

python3 - "$RELEASE_DIR" "$VERSION" "$TARGET" <<'PYEOF'
import hashlib
import json
import os
import sys
import binascii

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization

release_dir = sys.argv[1]
version = sys.argv[2]
target = sys.argv[3]
seed_hex = os.environ["DEVE_SUB_RELEASE_KEY_SEED"]

seed = binascii.unhexlify(seed_hex)
if len(seed) != 32:
    print("ERROR: seed must be 32 bytes", file=sys.stderr)
    sys.exit(1)

# WHY: Ed25519PrivateKey.from_private_bytes takes the 32-byte seed directly,
# matching ed25519_dalek::SigningKey::from_bytes. The resulting signature
# is byte-compatible with ed25519_dalek's verify.
key = Ed25519PrivateKey.from_private_bytes(seed)

assets = []
for name in sorted(os.listdir(release_dir)):
    if name in ("checksums.txt", "deve-sub-sbom.json",
                "deve-sub-manifest.json", "deve-sub-manifest.json.sig"):
        continue
    path = os.path.join(release_dir, name)
    if not os.path.isfile(path):
        continue
    with open(path, "rb") as f:
        sha = hashlib.sha256(f.read()).hexdigest()
    size = os.path.getsize(path)
    assets.append({"name": name, "sha256": sha, "size": size})

manifest = {"version": version, "target": target, "assets": assets}
manifest_json = json.dumps(manifest, separators=(",", ":")).encode("utf-8")

signature = key.sign(manifest_json)

manifest_path = os.path.join(release_dir, "deve-sub-manifest.json")
sig_path = os.path.join(release_dir, "deve-sub-manifest.json.sig")

with open(manifest_path, "wb") as f:
    f.write(manifest_json)
with open(sig_path, "wb") as f:
    f.write(signature)

print(f"signed manifest for {len(assets)} assets -> {manifest_path}")
print(f"signature ({len(signature)} bytes) -> {sig_path}")
PYEOF
