# Milestone 11 — Archive and Snapshot

## Scope

Full-state backup and restore: database snapshot, configuration export, and
CLI-driven restore. M11 delivers the `deve-sub backup` and `deve-sub restore`
CLI subcommands (reserved in AGENTS.md naming section), a snapshot format that
captures the complete application state (SQLite database + master key
reference + metadata), and a restore-verify path.

M11 is a **low-priority** milestone. It is scheduled after M9 and M10 but may
be deferred further if higher-priority work (M8 deployment hardening, client
compatibility validation OUT-001 through OUT-007) takes precedence.

## Dependency

M1 (Infrastructure) must be complete. The SQLite pool, migration framework,
and CLI infrastructure are prerequisites.

M6 (Subscription Distribution) must be complete. The backup must capture
subscriptions, templates, traffic records, and probe data — all of which land
by M6/M7.

M7 (Probes and Detection) must be complete. Probe sources, latency records,
and dashboard data are part of the application state.

M10 (Observability and Audit) should be complete. Audit logs and traffic
daily snapshots are part of the application state; backing them up is more
valuable after they exist.

## Vertical slice

```text
deve-sub backup --output /path/to/backup.tar
    → snapshots the SQLite database (VACUUM INTO or .backup)
    → exports configuration (server config, master key reference)
    → writes a versioned backup manifest
    → writes backup.tar

deve-sub restore --input /path/to/backup.tar
    → reads backup manifest, verifies version compatibility
    → stops the server (or refuses if server is running)
    → restores the SQLite database
    → runs migration check (forward-only; if backup is older than current
      schema, migrations run forward)
    → verifies restore (row counts, schema integrity)
```

## Deliverables

- Backup format: versioned tar archive containing:
  - `manifest.json` — backup version, schema version, timestamp, row counts
  - `database.sqlite` — snapshot of the SQLite database (VACUUM INTO)
  - `config.json` — non-secret configuration (server bind address, port, etc.)
  - `metadata.json` — backup metadata (Deve Sub version, git commit, host)
- CLI: `deve-sub backup --output <path>` (creates backup), `deve-sub restore
  --input <path>` (restores from backup).
- Restore verification: after restore, the CLI runs a verification pass
  (schema integrity check, row count comparison against manifest, migration
  version check) and reports discrepancies.
- Migration handling: if the backup's schema version is older than the
  current binary's schema version, `restore` runs forward migrations after
  restoring the database. If the backup's schema is newer, `restore` refuses
  with an error (forward-only migration policy — constraint #13).
- Server lock: `restore` refuses to run while the server is running (checks
  for a PID file or SQLite lock). `backup` can run while the server is
  running (SQLite snapshot is consistent via `VACUUM INTO`).
- Documentation: backup/restore guide in `docs/guides/`.

## Slicing

M11 is delivered in two slices:

1. **Backup**: `deve-sub backup` CLI command, backup format, SQLite VACUUM
  INTO snapshot, manifest and metadata generation. Acceptance: BACKUP-001
  (backup creation), BACKUP-002 (backup contents verification).
2. **Restore**: `deve-sub restore` CLI command, backup parsing, database
  restore, forward migration handling, verification pass, server lock.
  Acceptance: BACKUP-003 (restore + verification), BACKUP-004 (forward
  migration on restore from older schema).

## Architecture

### Backup format

```text
backup.tar
├── manifest.json     # { "version": 1, "schema_version": 14, "created_at": "...", "row_counts": {...} }
├── database.sqlite   # VACUUM INTO snapshot (consistent, no locks held)
├── config.json       # non-secret server configuration
└── metadata.json     # { "deve_sub_version": "...", "git_commit": "...", "host": "...", "os": "..." }
```

The backup is a tar archive, not a directory, to ensure atomicity (single
file to copy, transfer, or upload). The manifest carries the schema version
so `restore` can decide whether to run migrations.

### SQLite snapshot

`VACUUM INTO '/path/to/snapshot.sqlite'` produces a consistent snapshot
without holding write locks on the main database. This allows `backup` to run
while the server is serving requests. The snapshot includes all tables,
indexes, and triggers.

### Restore and migration

```text
restore(backup_path):
  1. read manifest.json → get backup schema version
  2. check server is not running (PID file / SQLite lock)
  3. copy backup database.sqlite to the configured database path
  4. if backup schema < current schema:
       run forward migrations (constraint #13: forward-only)
  5. if backup schema > current schema:
       refuse with error ("backup is from a newer version")
  6. verify: row counts vs manifest, schema integrity (PRAGMA integrity_check)
  7. report result
```

## Failure/recovery

- Backup failure (disk full, permission denied): the partial backup file is
  deleted. The error is reported. The running server is unaffected.
- Restore failure (corrupt backup, wrong format): the existing database is
  not touched. The error is reported. The user must resolve the backup issue
  and retry.
- Restore with server running: `restore` refuses and reports the PID. The
  user must stop the server first.
- Schema version mismatch (backup newer than binary): `restore` refuses. The
  user must upgrade the binary to match or exceed the backup's schema version.
- Migration failure during restore: the database is left in the
  pre-migration state (the restored backup). The error is reported. The user
  can retry with a newer binary or inspect the migration error.

## Authority

- Forward-only migration policy: constraint #13
- CLI subcommand names: AGENTS.md naming section (`backup`, `restore`)
- Storage policy: `docs/plan/13-storage.md`
- Acceptance: BACKUP-001 through BACKUP-004

## Verification

- Backup creation: run `deve-sub backup --output /tmp/test.tar`, verify the
  archive contains manifest, database, config, metadata. Acceptance:
  BACKUP-001.
- Backup contents: open the snapshot database, verify row counts match the
  manifest. Acceptance: BACKUP-002.
- Restore: run `deve-sub restore --input /tmp/test.tar` on a fresh instance,
  verify the database is restored and the server starts. Acceptance:
  BACKUP-003.
- Forward migration: create a backup from an older schema version, restore on
  a newer binary, verify migrations run forward and data is intact.
  Acceptance: BACKUP-004.
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all pass.
- `python3 scripts/check_docs.py` passes.
