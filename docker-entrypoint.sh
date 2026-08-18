#!/bin/sh
# Deve Sub Docker entrypoint (DS-AUD-007): run idempotent migrations before
# starting the server so `docker run` works without a separate migrate step.
set -e

# WHY: generate the master key on first boot if it does not exist. The serve
# command loads the key via MasterKey::load (strict, fail-closed) per
# ADR-0007 §7 — allow_master_key_generation defaults to false to prevent
# silent key rotation when a production mount is lost. Generating the key
# here (not inside serve) keeps the fail-closed invariant: the key file
# always exists before serve starts, and the key is persisted in the same
# volume as the SQLite database, so they stay paired. If the volume is lost,
# both the key and the database are lost together — no key/DB mismatch. This
# mirrors the standard pattern for stateful images (e.g. PostgreSQL first-boot
# secret generation). 32 bytes from /dev/urandom satisfies the KEY_LEN
# requirement in deve-sub-security::master_key.
MASTER_KEY_PATH="/app/data/master.key"
if [ ! -f "$MASTER_KEY_PATH" ]; then
    head -c 32 /dev/urandom > "$MASTER_KEY_PATH"
    chmod 600 "$MASTER_KEY_PATH"
fi

/app/deve-sub migrate --db-path /app/data/deve-sub.db

exec /app/deve-sub serve \
    --db-path /app/data/deve-sub.db \
    --web-dist-dir /app/web/dist
