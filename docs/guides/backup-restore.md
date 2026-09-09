# Backup and Restore Guide

## Overview

Deve Sub provides `deve-sub backup` and `deve-sub restore` CLI subcommands for
full-state backup and restore. The backup captures the SQLite database,
non-secret configuration, and environment metadata in a versioned tar archive.

## Backup

### Creating a backup

```bash
deve-sub backup --output /path/to/backup.tar
```

Optional flags:

- `--config <path>` — configuration file (defaults to `DEVE_SUB_CONFIG` env or
  built-in search).
- `--db-path <path>` — database path (overrides config).

The backup runs while the server is online. SQLite `VACUUM INTO` produces a
consistent snapshot without holding a write lock.

### Archive contents

| File | Description |
|---|---|
| `manifest.json` | Format version, schema version, timestamp, row counts |
| `database.sqlite` | VACUUM INTO snapshot of the SQLite database |
| `config.json` | Non-secret configuration (bind address, port, paths) |
| `metadata.json` | Binary version, git commit, host, OS |

### What the backup includes

The `database.sqlite` snapshot contains the **full production database**:
users, TOTP secrets, session tokens, subscription URLs, encrypted sensitive
fields, audit logs, traffic snapshots, probe data, and templates.

Treat backup archives as sensitive. Store them with restrictive permissions
and never distribute them off-box.

## Restore

### Restoring from a backup

```bash
# Stop the server first
deve-sub restore --input /path/to/backup.tar
```

Optional flags:

- `--config <path>` — configuration file.
- `--db-path <path>` — database path to restore into (overrides config).

### Restore behavior

The authoritative sequence is in [M11: Restore and migration](../plan/milestones/M11-archive-and-snapshot.md#restore-and-migration).
The CLI verifies key continuity and the archived row counts before applying
forward migrations to a staging database. It checks integrity before the atomic
replacement. A failed check leaves the existing database intact. Stop `serve`
first; the guard uses a sidecar flock, not the mere existence of WAL/SHM files.

### Restoring from an older schema

If the backup was created with an older schema version, `restore`
automatically runs forward migrations after copying the snapshot:

```bash
deve-sub restore --input /old-backup.tar
# Output:
#   Restore complete.
#   database: /var/lib/deve-sub/deve-sub.db
#   schema version: 13 -> 14
#   integrity: ok
```

## Scheduling

For automated backups, use cron or systemd timers:

```bash
# Daily backup at 03:00
0 3 * * * /usr/local/bin/deve-sub backup --output /backups/deve-sub-$(date +\%Y\%m\%d).tar
```

Retain old backups according to your recovery point objective. Back up the
master key separately and provide the matching key when restoring encrypted
data; the archive contains its reference/fingerprint, not the secret key.
A downgrade needs the pre-upgrade archive and the corresponding older binary.
Lifetime traffic projections remain in a backup after recent raw rows expire.
