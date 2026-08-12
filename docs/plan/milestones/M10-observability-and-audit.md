# Milestone 10 — Observability and Audit

## Scope

Traffic history charts (daily traffic snapshots with a background aggregation
job, a history query API, and dashboard chart data), and the audit log query
API (the `audit_log` table exists since migration 0002 but has no application
infrastructure — M10 builds the domain model, repository, write-side wiring
into key mutating commands, and the read-side query API).

M10 delivers: the `deve-sub-domain` `audit` module, the `deve-sub-application`
`audit` module and `traffic_history` extension, storage adapters, the
`/api/v1/audit-logs` and `/api/v1/dashboard/traffic/history` REST API surfaces,
and a daily traffic snapshot background job (observable, cancellable, safe
shutdown — constraint #20).

## Dependency

M6 (Subscription Distribution) must be complete. The `subscription_traffic`
table (migration 0011) and `TrafficRepository` port are prerequisites: M10
adds a daily snapshot table and aggregation job on top of the existing
append-only traffic records.

M7 (Probes and Detection) must be complete. Probe traffic records
(`source_kind = Probe`) feed the daily aggregation. The dashboard traffic API
from M7 (`/api/v1/dashboard/traffic`) is extended with a history endpoint.

M2 (Auth and Users) must be complete. The `audit_log` table (migration 0002)
was created with the initial schema but never wired to application code. M10
builds the missing infrastructure and wires audit log writing into auth and
key CRUD commands.

M1 (Infrastructure) provides the SQLite pool, migration framework, Axum server,
background job infrastructure (probe runner pattern reused), and CLI.

## Vertical slice

```text
GET /api/v1/audit-logs?action=login&limit=50&cursor=...
    → returns paginated audit log entries (actor, action, target, details, time)

GET /api/v1/dashboard/traffic/history?subscription_id=...&days=30
    → returns daily traffic snapshots [{date, upload, download, breakdown}]

# Background job (daily):
aggregate_traffic_snapshots()
    → for each subscription, sum traffic records for the previous day
    → upsert traffic_daily_snapshots row
    → observable via job status, cancellable, safe shutdown (constraint #20)
```

## Deliverables

- Domain audit module: `AuditLog` aggregate (id, actor_id, action,
  target_type, target_id, details_json, created_at), `AuditLogRepository` port
  trait (insert, list with filters and cursor pagination).
- Domain traffic history extension: `TrafficDailySnapshot` value object
  (subscription_id, date, total_upload, total_download, source_breakdown),
  `TrafficDailySnapshotRepository` port trait (upsert, list by subscription
  and date range).
- Application audit module: `record_audit_log` command (called by other
  commands at mutation points), `list_audit_logs` query (filters: actor_id,
  action, target_type, target_id, date range; cursor pagination by created_at).
- Application traffic history extension: `aggregate_daily_traffic` background
  job (daily, observable, cancellable, safe shutdown — constraint #20),
  `list_traffic_history` query.
- Audit log wiring: `record_audit_log` calls injected into auth commands
  (login, logout, 2FA enable/disable, password change), user CRUD, source CRUD,
  subscription CRUD, probe source CRUD. Each call records actor_id (from
  session context), action string (e.g. `"user.create"`, `"source.refresh"`),
  target_type + target_id, and details_json (non-sensitive metadata only).
- Storage adapter: `SqliteAuditLogRepository`, `SqliteTrafficDailySnapshotRepository`.
- Migration 0014: `traffic_daily_snapshots` table (subscription_id, date,
  total_upload, total_download, source_breakdown_json, computed_at; UNIQUE
  constraint on `(subscription_id, date)` for idempotent upsert). No migration
  needed for `audit_log` (table exists since 0002).
- Server: audit log query route (`GET /api/v1/audit-logs`, admin-only),
  traffic history route (`GET /api/v1/dashboard/traffic/history`, admin-only).
  Both utoipa-documented.
- Contract DTOs: audit log entry, audit log query params, traffic daily
  snapshot, traffic history query params. All DTOs and `ToSchema` derives in
  `deve-sub-contract` per ADR-0004.
- CLI: `deve-sub audit list` (optional, admin-only).

## Slicing

M10 is delivered in three slices:

1. **Audit log infrastructure + auth wiring**: `AuditLog` domain model,
   `AuditLogRepository` port, `SqliteAuditLogRepository` adapter,
   `record_audit_log` command, `list_audit_logs` query, `GET /api/v1/audit-logs`
   API (admin-only, cursor pagination, filters), wire audit log writing into
   auth commands (login, logout, 2FA, user CRUD). Acceptance: AUDIT-001
   (query with filters and pagination), AUDIT-002 (auth actions audited).
2. **Audit log wiring for remaining commands**: wire `record_audit_log` into
   source CRUD, subscription CRUD, template CRUD, probe source CRUD. Each
   mutation records an audit entry. Acceptance: AUDIT-003 (CRUD actions
   audited across modules).
3. **Traffic history snapshots + history API**: migration 0014
   (`traffic_daily_snapshots`), `TrafficDailySnapshot` domain model,
   `TrafficDailySnapshotRepository` port + SQLite adapter, daily aggregation
   background job (observable, cancellable, safe shutdown — constraint #20),
   `list_traffic_history` query, `GET /api/v1/dashboard/traffic/history` API
   (admin-only). Acceptance: TRAFFIC-001 (daily snapshot aggregation),
   TRAFFIC-002 (history query, 30-day chart data).

## Architecture

### Audit log model

```text
AuditLog {
  id: AuditLogId,
  actor_id: Option<UserId>,    // None for system/anonymous actions
  action: String,              // e.g. "user.create", "source.refresh", "auth.login"
  target_type: Option<String>, // e.g. "user", "source", "subscription"
  target_id: Option<String>,   // ID of the target entity
  details_json: Option<String>,// non-sensitive metadata (no secrets, no tokens)
  created_at: Timestamp,
}
```

The `audit_log` table is append-only. No updates, no deletes (except via
retention policy, if added later). The `actor_id` foreign key has
`ON DELETE SET NULL` — deleting a user preserves audit history with the actor
anonymized.

### Audit log write-side wiring

```text
// In each mutating command:
async fn create_source(ctx: &CommandContext, req: CreateSourceRequest) {
    let source = source_repo.insert(...).await?;
    ctx.audit_log.record(AuditLogEntry {
        actor_id: ctx.session.user_id,
        action: "source.create",
        target_type: Some("source"),
        target_id: Some(&source.id),
        details: json!({"name": source.name, "kind": source.kind}),
    }).await?;
    Ok(source)
}
```

The `CommandContext` carries an `AuditLogRepository` reference. Commands call
`record` after a successful mutation. Audit log write failure is logged but
does not fail the parent command (best-effort, non-blocking — audit is
observability, not a transactional side-effect).

### Audit log action naming

Action strings follow `{module}.{verb}` convention:

```text
auth.login, auth.logout, auth.2fa.enable, auth.2fa.disable,
auth.password.change, auth.user.create, auth.user.update, auth.user.delete,
auth.user.disable, auth.user.enable,
source.create, source.update, source.delete, source.refresh,
subscription.create, subscription.update, subscription.delete,
subscription.token.rotate,
template.create, template.update, template.delete, template.rollback,
probe.source.create, probe.source.update, probe.source.delete,
probe.source.sync, probe.run.start, probe.run.cancel,
```

### Traffic daily snapshot model

```text
TrafficDailySnapshot {
  subscription_id: SubscriptionId,
  date: Date,                  // YYYY-MM-DD (UTC)
  total_upload: u64,           // sum of all traffic records for this day
  total_download: u64,
  source_breakdown: BTreeMap<TrafficSourceKind, (u64, u64)>,
                                // per-source-kind (upload, download)
  computed_at: Timestamp,
}
```

### Traffic daily aggregation job

```text
aggregate_daily_traffic():
  → runs daily (configurable schedule, default 00:30 UTC)
  → for each subscription with traffic records:
      → query subscription_traffic WHERE recorded_at >= start_of_yesterday
        AND recorded_at < start_of_today
      → group by source_kind, sum upload/download
      → upsert traffic_daily_snapshots (subscription_id, date=yesterday)
  → observable via job status (last_run_at, records_processed, errors)
  → cancellable via CancellationToken
  → safe shutdown (joins with timeout, constraint #20)
```

The aggregation reads the raw `subscription_traffic` records and sums per
source kind. For cumulative-counter sources (Nezha, Komari), each record
already stores a delta (computed by the adapter at sync time), so summing
deltas gives the day's total. For AirportHeader records (cumulative totals
from `subscription-userinfo`), the aggregation computes the delta between the
last record of the day and the last record of the previous day. Manual
correction records are summed directly.

### Traffic history query

```text
GET /api/v1/dashboard/traffic/history?subscription_id=...&days=30
    → returns [{date, total_upload, total_download, source_breakdown}]
    → if subscription_id omitted, returns global aggregate across all subscriptions
    → days: 1-365 (default 30)
```

The query reads from `traffic_daily_snapshots`. For days with no snapshot
(zero traffic or job not yet run), the API fills gaps with zero-value entries
so the chart is continuous.

## Failure/recovery

- Audit log write failure: the `record` call is best-effort. If the repository
  insert fails (e.g. DB error), the error is logged at `warn` level but the
  parent command succeeds. Audit is observability infrastructure, not a
  transactional side-effect. Losing an audit entry is preferable to failing a
  user-facing operation.
- Daily aggregation job failure: the job records its failure status
  (last_run_at, error message). The next run retries the previous day's
  aggregation. Missing snapshots appear as zero-value gaps in the history
  query. The job is idempotent (upsert on `(subscription_id, date)`).
- Daily aggregation job crash: on restart, the job checks for missing days
  since the last successful run and backfills. If the gap exceeds a
  configurable threshold (default 7 days), it logs a warning and only
  backfills the most recent 7 days (older data is accepted as lost).
- Migration 0014 has a recovery test (constraint #13): apply migration, verify
  schema, restore from pre-migration backup, verify rollback.
- Server shutdown during aggregation: the job joins with a configurable
  timeout (default 5s). In-progress aggregation is abandoned; the next run
  retries. No partial snapshots are committed (upsert is atomic per
  subscription-day).

## Authority

- Audit log model: this blueprint §"Audit log model"
- Audit log table schema: `migrations/0002_initial.sql` (existing, unchanged)
- Traffic daily snapshot model: this blueprint §"Traffic daily snapshot model"
- Traffic data model: `docs/plan/01-terminology.md` §"Traffic"
- Traffic record infrastructure: M6 blueprint §"Traffic and expiry policy
  framework", M7 blueprint §"Traffic aggregation"
- Background job discipline: constraint #20 (observable, cancellable, safe
  shutdown)
- DTO ownership: ADR-0004 (DTOs + `ToSchema` in `deve-sub-contract`)
- Module boundaries: `docs/contracts/module-boundaries.md` (amended in M10 to
  name the audit log and traffic history surfaces)
- Acceptance: AUDIT-001 through AUDIT-003, TRAFFIC-001, TRAFFIC-002

## Verification

- Audit log query: perform several audited actions (login, create user, create
  source), then query `GET /api/v1/audit-logs` with filters and verify
  paginated results. Acceptance: AUDIT-001.
- Auth actions audited: login, logout, enable 2FA, create/update/delete user;
  verify each produces an `audit_log` row with correct actor, action, and
  target. Acceptance: AUDIT-002.
- CRUD actions audited: create/update/delete for source, subscription,
  template, probe source; verify audit entries. Acceptance: AUDIT-003.
- Daily traffic snapshot: insert traffic records for a subscription, run the
  aggregation job, verify `traffic_daily_snapshots` row with correct sums and
  source breakdown. Acceptance: TRAFFIC-001.
- Traffic history query: populate multiple days of snapshots, query
  `GET /api/v1/dashboard/traffic/history?days=30`, verify continuous daily
  data with correct values and gap-filling. Acceptance: TRAFFIC-002.
- Migration 0014 recovery test. Acceptance: DEPLOY-001 (migration subset).
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all pass.
- `python3 scripts/check_docs.py` passes.
- OpenAPI spec regenerated and up to date.
