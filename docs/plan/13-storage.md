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
- Batch large imports with bounded SQL statements. Source reconciliation and
  snapshot publication remain one atomic transaction.
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

## Production WAL lifecycle

The SQLite adapter owns a PASSIVE checkpoint operation. `serve` calls it at
startup and every 60 seconds, logging busy/log/checkpointed frame counts plus
main-database and WAL bytes; incomplete checkpoints and failures are visible.
Automatic checkpointing at 1,000 pages remains enabled. A 16 MiB
`journal_size_limit` limits retained allocation after WAL reset, not live WAL.
Long-lived readers can still pin frames: no safe non-blocking checkpoint can
promise an absolute WAL-size ceiling in that case. Periodic TRUNCATE is
forbidden because it may stall readers/writers. Shutdown stops new work and
workers, attempts a final PASSIVE checkpoint, then closes the pool. Backup
uses its existing online snapshot boundary and does not depend on copying a
checkpointed main database. Repeated-write and pinned-reader tests exercise
PERF-006's normal-envelope and recovery behavior.

## Historical data lifecycle (0024)

| Data | Policy | Cleanup owner |
| --- | --- | --- |
| Traffic raw deltas | 30 days; lifetime totals survive pruning | SQLite maintenance |
| Traffic daily snapshots | 400 days | SQLite maintenance |
| Lifetime/probe traffic totals | Entity lifetime; at most three known source kinds per subscription (legacy probe prefixes preserved) | Subscription cascade |
| Completed/failed/cancelled probe and source refresh runs | 30 days; active runs excluded; crash recovery and legacy terminal rows lacking completion time start a new diagnostic window | SQLite maintenance |
| Expired sessions and temporary links | Remove after expiry | SQLite maintenance |
| Processed outbox events | 30 days; unprocessed events never pruned | SQLite maintenance |
| Source snapshots | Existing last 10 versions per source | Source publication |
| Generation cache | Existing active version plus 8 inactive entries per template/profile | Generation publication |
| Audit log | Intentional lifetime retention for accountability; indexed cursor queries | Operator-managed archival |
| Template versions, missing nodes, live entities | Intentional user-owned state; no automatic destructive expiry | Explicit entity commands |

Maintenance performs 500 parent-row deletions per table per round, at most
10 fair rounds per minute within a 10-second outer deadline and indexed cutoffs. Cascaded probe results can
make a batch larger than 500 physical rows. Deleted space is reused; no routine
VACUUM or shrinking promise is made. Arrival rate above cleanup throughput,
long read transactions and disk exhaustion remain observable operational limits.
No cleanup of a user's existing database is performed by development commands.
