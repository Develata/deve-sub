# 13 — Storage

## Scope

This chapter defines the database strategy, SQLite configuration, the storage
Port pattern, and the migration policy. See ADR-0002 for the storage Port
SQLite-first decision.

## Database strategy

```text
Default deployment: SQLite + WAL
Multi-instance deployment: PostgreSQL (later version)
```

SQLite suits single-machine application servers and low-to-medium concurrency.
WAL mode allows concurrent reads with a single writer; write transactions must
stay short.

PostgreSQL suits multi-instance, higher write concurrency, and high
availability. Its MVCC reduces read/write lock contention and provides
replication and failover.

SQLx supports both SQLite and PostgreSQL with connection pooling, migration,
and optional compile-time query checking.

The first version does not maintain two SQL sets. The architecture defines a
storage Port; the SQLite adapter is implemented first. PostgreSQL is a later
milestone.

## SQLite configuration

```text
journal_mode=WAL
foreign_keys=ON
busy_timeout=5000
synchronous=NORMAL
temp_store=MEMORY
```

## Requirements

- Keep write transactions short.
- Batch large node imports in chunked transactions.
- Configure periodic WAL checkpoints.
- Monitor WAL size.
- Do not place SQLite on NFS or network volumes.
- Docker data directory must be a local persistent volume.
- Use the online backup API or `VACUUM INTO` for backups.
- Never copy the database main file while running as a backup.
- Support database integrity checks.
- Support pre-migration rollback backup.

## Migration policy

- `migrations/` is the physical schema source of truth.
- Each database change has a migration and a recovery test. See constraint
  #13.
- Migrations are forward-only; rollback is achieved by restoring a pre-migration
  backup.
- `docs/data-model/` is the conceptual entity model; migrations are the
  physical source of truth.

## Authority

- Storage Port decision: ADR-0002
- Conceptual model: `docs/data-model/core-er.md`
- Physical schema: `migrations/`

## Verification

- Each migration has a recovery test. Acceptance: `DEPLOY-001`.
- WAL and memory do not grow unbounded over long runs. Acceptance: `PERF-006`.
