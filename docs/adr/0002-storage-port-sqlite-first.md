# ADR-0002: Storage Port — SQLite-first

- **Status**: Accepted
- **Date**: 2026-08-02

## Context

Deve Sub needs persistent storage for sources, nodes, snapshots, overrides,
templates, subscriptions, users, sessions, audit logs, and outbox events. The
data is highly relational: pagination, search, audit, permissions, statistics,
and version queries are common.

The default deployment is a single-machine application server with
low-to-medium concurrency. A multi-instance deployment with higher write
concurrency is a future option.

## Decision

Define a **storage Port** in the domain/application layer. Implement the
**SQLite adapter first**. PostgreSQL is a later milestone, not a P0 dependency.

- SQLite + WAL for the default deployment.
- The first version maintains a single SQL set (SQLite).
- PostgreSQL adapter is added later behind the same Port interface.
- redb is **not** used as the main database.

## Consequences

- Single SQL set for v1 reduces maintenance burden.
- Clear Port boundary: the domain and application layers do not know which
  database is in use.
- SQLite WAL allows concurrent reads with a single writer; write transactions
  must stay short.
- PostgreSQL remains a non-blocking future option.

## Alternatives considered

1. **redb** — rejected: it is a stable embedded ACID key-value store, but KV is
   a poor fit for the relational, paginated, searchable, audited data in this
   project. Using redb would force manual secondary indexes, relation
   management, and migration machinery.
2. **PostgreSQL from the start** — rejected: adds operational complexity
   (separate process, replication setup) for the default single-machine
   deployment.
3. **Two SQL sets simultaneously** — rejected: doubles query maintenance and
   testing burden for v1 with no immediate benefit.
