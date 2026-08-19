#!/bin/sh
# Deve Sub Docker entrypoint (DS-AUD-007): run idempotent migrations before
# starting the server so `docker run` works without a separate migrate step.
set -e

# WHY (DS-AUD-B01): serve loads the key via MasterKey::load (strict,
# fail-closed) per ADR-0007 §7 — allow_master_key_generation defaults to
# false to prevent silent key rotation when a production mount is lost.
# Generating the key here (not inside serve) keeps the fail-closed
# invariant: the key file always exists before serve starts, and the key
# is persisted in the same volume as the SQLite database, so they stay
# paired. If the volume is lost, both the key and the database are lost
# together — no key/DB mismatch. This mirrors the standard pattern for
# stateful images (e.g. PostgreSQL first-boot secret generation).
#
# We use the `deve-sub key init` subcommand (same code path the native
# install.sh uses) rather than `head -c 32 /dev/urandom` so the new
# command gets real runtime coverage and the two deploy paths cannot
# drift. The shell guard keeps it idempotent on container restart with a
# persistent volume — `key init` refuses if the key already exists, which
# is the right behavior for a manual invocation but would break the
# container's restart contract.
MASTER_KEY_PATH="/app/data/master.key"
if [ ! -f "$MASTER_KEY_PATH" ]; then
    /app/deve-sub key init \
        --key-path "$MASTER_KEY_PATH" \
        --db-path /app/data/deve-sub.db
fi

/app/deve-sub migrate --db-path /app/data/deve-sub.db

exec /app/deve-sub serve \
    --db-path /app/data/deve-sub.db \
    --key-path /app/data/master.key \
    --bind 0.0.0.0:8080 \
    --web-dist-dir /app/web/dist
