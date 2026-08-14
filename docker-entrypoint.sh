#!/bin/sh
# Deve Sub Docker entrypoint (DS-AUD-007): run idempotent migrations before
# starting the server so `docker run` works without a separate migrate step.
set -e

/app/deve-sub migrate --db-path /app/data/deve-sub.db

exec /app/deve-sub serve \
    --db-path /app/data/deve-sub.db \
    --web-dist-dir /app/web/dist
