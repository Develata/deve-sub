#!/usr/bin/env bash
# Build the Dioxus web frontend and normalize the output to apps/web/dist/.
#
# WHY: Dioxus 0.7 emits to target/dx/{name}/release/web/public by default,
# but CI, Dockerfile, and Playwright all expect apps/web/dist/. Centralizing
# the build + normalize + verify here means the internal Dioxus path appears
# in exactly one place, and every consumer gets a validated dist (DS-AUD-004,
# DS-AUD-007, DS-AUD-053).
#
# Usage:
#   scripts/build-web-release.sh          # build + verify
#   DEVE_SUB_DX_SKIP_VERIFY=1 scripts/build-web-release.sh  # build only
set -euo pipefail

cd "$(dirname "$0")/.."

# WHY: --features frontend is required because the web bin target declares
# required-features = ["frontend"]. Without it the bin is not built and dx
# produces an empty/placeholder output.
dx build --release --package deve-sub-web --features frontend

SRC="target/dx/deve-sub-web/release/web/public"
DST="apps/web/dist"

if [ ! -d "$SRC" ]; then
  echo "FAIL: Dioxus output not found at $SRC" >&2
  echo "      Check whether dx build succeeded or the output path changed." >&2
  exit 1
fi

rm -rf "$DST"
mkdir -p "$DST"
cp -r "$SRC/." "$DST/"

if [ "${DEVE_SUB_DX_SKIP_VERIFY:-0}" != "1" ]; then
  # Contract: index.html + WASM bundle + JS glue must all exist.
  test -f "$DST/index.html" || { echo "FAIL: $DST/index.html missing" >&2; exit 1; }
  ls "$DST/assets/"*.wasm >/dev/null 2>&1 || { echo "FAIL: no .wasm in $DST/assets/" >&2; exit 1; }
  ls "$DST/assets/"*.js >/dev/null 2>&1 || { echo "FAIL: no .js in $DST/assets/" >&2; exit 1; }
  echo "Web frontend built and verified at $DST"
else
  echo "Web frontend built at $DST (verification skipped)"
fi
