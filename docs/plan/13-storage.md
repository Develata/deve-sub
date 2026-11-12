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

### Encrypted generation cache (0029)

Generated subscriptions contain usable proxy credentials. Schema 0029 renames
the old payload column to `content_legacy` and adds `content_encrypted`.
New cache writes use the existing v2 XChaCha20-Poly1305 envelope with context
bound to `generation_cache.content`, the row ID and cache key. The repository
requires the existing master key. Reads authenticate ciphertext and reject
pending legacy rows, malformed envelopes, wrong keys and tampering; they never
retry the legacy column as plaintext. Activation authenticates its target
before replacing an active result.

After stopping the service and retaining a pre-upgrade backup, run
`deve-sub migrate --key-path /path/to/existing/master.key` (or set
`DEVE_SUB_KEY_PATH`). A nonempty cache requires the existing key; the default
path is `data/master.key`. Fresh empty caches can migrate before key bootstrap.
Schema migration may finish before keyed payload migration. The payload phase
authenticates existing ciphertext, binds/verifies the key and encrypts legacy
rows in one transaction. Failure rolls back that phase; retry with the correct
key. `serve` refuses to start with pending payload migration.

Backfill preserves cache IDs, request keys, selectors, version pins, modes and
active flags. It does not invalidate every last-good result or bypass the pool
revision floor. Retention still protects the latest matching lenient result
for each persisted subscription, as described below. Backup rejects pending
plaintext caches and checks its immutable SQLite snapshot. Restore authenticates
and migrates a staging database before swapping it into place.

This is a forward schema change: an older binary cannot read the encrypted
cache or safely run against schema 0029. Rollback requires the older binary
and a compatible pre-upgrade backup with its original key; deleting the new
column is not rollback. Prior backups and old SQLite/WAL/free-page copies can
still contain historical plaintext. Migration is not a secure-erasure
operation. Existing short codes are also cleartext bearer credentials, so
protect database files and backups even after cache encryption.

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
| Generation cache | Active result, latest matching lenient result for each existing subscription's selection/version pin, plus 8 unprotected inactive entries per template/profile | Generation publication |
| Audit log | 90 days by default; 0 disables expiry; explicit preview/confirmation for manual cleanup | Application audit commands via the audit Port; atomic cleanup receipt; see M10 |
| Template versions, missing nodes, live entities | Intentional user-owned state; no automatic destructive expiry | Explicit entity commands |

Maintenance performs 500 parent-row deletions per table per round, at most
10 fair rounds per minute within a 10-second outer deadline and indexed cutoffs. Cascaded probe results can
make a batch larger than 500 physical rows. Deleted space is reused; no routine
VACUUM or shrinking promise is made. Arrival rate above cleanup throughput,
long read transactions and disk exhaustion remain observable operational limits.
No cleanup of a user's existing database is performed by development commands.
