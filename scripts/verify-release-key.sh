#!/usr/bin/env bash
# Verify that the DEVE_SUB_RELEASE_KEY_SEED secret corresponds to the
# RELEASE_PUBLIC_KEY embedded in apps/cli/src/update_manifest.rs (P0-04).
#
# This runs in CI where the secret is available. It fails-closed: if the
# seed is missing or the derived public key does not match the embedded
# key, the job aborts — preventing a release signed by the wrong key.
#
# Usage (no arguments; reads DEVE_SUB_RELEASE_KEY_SEED from env):
#   DEVE_SUB_RELEASE_KEY_SEED=<hex> scripts/verify-release-key.sh

set -euo pipefail

if [ -z "${DEVE_SUB_RELEASE_KEY_SEED:-}" ]; then
    echo "ERROR: DEVE_SUB_RELEASE_KEY_SEED is not set" >&2
    exit 1
fi

if ! echo "$DEVE_SUB_RELEASE_KEY_SEED" | grep -qE '^[0-9a-fA-F]{64}$'; then
    echo "ERROR: DEVE_SUB_RELEASE_KEY_SEED must be 64 hex chars (32 bytes)" >&2
    exit 1
fi

# Extract the embedded RELEASE_PUBLIC_KEY from the Rust source as a hex
# string. The constant is a two-line array of 0xNN byte literals.
EMBEDDED_KEY_HEX=$(sed -n '/const RELEASE_PUBLIC_KEY:/,/];/p' \
    apps/cli/src/update_manifest.rs \
    | grep -oE '0x[0-9a-fA-F]{2}' \
    | sed 's/0x//' \
    | tr -d '\n')

if [ -z "$EMBEDDED_KEY_HEX" ]; then
    echo "ERROR: could not extract RELEASE_PUBLIC_KEY from source" >&2
    exit 1
fi

python3 - "$DEVE_SUB_RELEASE_KEY_SEED" "$EMBEDDED_KEY_HEX" <<'PYEOF'
import binascii
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization

seed_hex = sys.argv[1]
embedded_hex = sys.argv[2]

seed = binascii.unhexlify(seed_hex)
key = Ed25519PrivateKey.from_private_bytes(seed)

derived_pub = key.public_key().public_bytes(
    encoding=serialization.Encoding.Raw,
    format=serialization.PublicFormat.Raw,
).hex()

embedded_pub = embedded_hex.lower()

if derived_pub != embedded_pub:
    print(f"ERROR: seed-derived public key does not match embedded key", file=sys.stderr)
    print(f"  derived:  {derived_pub}", file=sys.stderr)
    print(f"  embedded: {embedded_pub}", file=sys.stderr)
    sys.exit(1)

print(f"OK: seed-derived public key matches embedded RELEASE_PUBLIC_KEY ({derived_pub})")
PYEOF
