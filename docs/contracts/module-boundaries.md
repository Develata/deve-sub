# Module Boundaries

## Scope

This contract defines the typed module boundaries, dependency direction, and
inter-module communication rules for Deve Sub.

### Audit lifecycle boundary (M10)

`AuditLogRepository` owns bounded candidate selection and atomic delete plus
receipt; application audit commands own retention validation and receipt
construction. Delivery never performs SQL. `GET /api/v1/audit-logs/policy`
reports effective retention days (0 disables automatic expiry) and batch size.
`POST /api/v1/audit-logs/cleanup/preview` accepts `keep_days` (1–3650), returns
`before_unix_ms`, ordered `entry_ids` (at most 500), and `has_more`.
`POST /api/v1/audit-logs/cleanup` accepts that cutoff and exact ID list, returns
`deleted` and `receipt_id`. Invalid scope is 400; changed candidates are 409;
timeout is 503. All three routes require an administrator and mutations use
same-origin CSRF protection. Cleanup scope is independent of viewer filters.
List time filters are inclusive `since` and exclusive `before`, RFC3339 timestamps
at whole-second precision. Configuration is server-owned and read-only in Web.

### Template input and history boundary (M5)

`spec_yaml` on template create/update accepts the original V3 document or
Clash/Mihomo routing YAML (`rules`, `proxy-groups`, `rule-providers`, `dns`,
`tun`), including a bare YAML rule list. Original text is preserved. Native
templates generate for `mihomo` only. No UI parsing or client conversion is
permitted. The optional internal `TemplateSpec.clash` stores validated native
sections as YAML text with mapping order preserved; absent means existing V3 behavior.

`TemplateRepository::update_with_version` allocates and returns the committed
history number atomically; it must exceed every existing version, including
after rollback. `GET /templates/{id}/versions/active` returns the active
version. History accepts an exclusive `before_version` cursor and returns
`next_before_version`; an absent cursor retains the first-page behavior.
Each page is bounded to 100 versions. Native validation errors return 400;
generation errors never replace the last successful output. Deleting a referenced
template returns 409 `template_in_use`. Pinned subscription cache fallback must
match both the requested version and generation mode.
`GenerationCacheRepository::store` atomically protects the newest matching
lenient result for each persisted subscription's selection/version pin, plus
the active result, while keeping at most eight additional inactive entries per
template/profile. Subscription edits/deletion release old protection on the
next store. Storage owns this retention transaction; no new delivery operation
or cross-repository application transaction is introduced.

The resolved selector bounds all group membership, including explicit and quick
groups, for both admin generation and delivery. Explicit out-of-selection nodes
are reported as `outside_selection`. Empty resolved Mihomo groups and unsupported
Mihomo group types fail before publication in both generation modes.
Unsupported groups return 422 `incompatible_groups` from generate/preview, with
the group names and types in the error message. Cache keys
include a generation semantics version; old active/fallback entries whose keys
do not match current semantics are unavailable until valid regeneration.

### Short-code replacement boundary (M6)

`ShortCodeRepository::replace` receives the new credential and replaces its
subscription's current short code atomically. It never trusts an old ID read by
the caller. Concurrent replacements preserve exactly one current credential;
failed writes preserve the previous credential and reference.

### Source refresh cancellation boundary (M4)

Manual and scheduled refreshes share the application cancellation registry.
The runner owns each registration until completion or drop. The cancel endpoint
returns 200 with `cancelled=true` when it signals the worker; clients continue
polling for its final outcome. Terminal jobs return `cancelled=false`. A live
job without a registered signal returns 503 `cancel_unavailable`, leaving the
job and source lease intact. Only the runner records cancellation after observing
it before publication; a refresh past that boundary completes normally.

The source scheduler observes shutdown within a tick, stops new admission and
drains already admitted workers. A lease acquisition that overlaps shutdown
finishes its durable transition and cancels before fetch. The CLI's bounded
grace/abort fallback still applies to workers that cannot finish normally.

### Probe history boundary (M7)

Probe results mark nodes absent at initial lookup as `skipped=true` with no RTT
or fabricated network error. `LatencyRecordRepository::batch_create` atomically
stores measurements for nodes still present at the write boundary, omitting
deleted nodes without losing other records. Measurements of subsequently
deleted nodes remain diagnostic run results. Other storage failures propagate:
the runner records Failed with collected results unless another terminal state
has already committed. Completed means eligible latency history was persisted.

### Independent source provenance (M4)

`NodePoolRepository::import_nodes` records independent provenance for inserts,
active duplicates and reactivations in its write transaction. Remote reconcile
cannot withdraw that contribution or replace manual overrides/tags. The storage
adapter uses the existing persisted node label for independent provenance and
live bindings for remote labels.

### Source cache invalidation (M4/M5)

`SourceRepository::update` and `delete` commit their pool-revision invalidation
atomically with the source mutation. Prior
generation semantics are not trusted as direct hits or last-good fallback.

Source deletion also withdraws orphaned remote-only nodes and advances the
pool-meta cache floor in this transaction. The generation-cache repository
enforces that floor on all reads, stores and activation; a rejected stale store
or activation cannot disturb a newer valid result. This boundary survives
restarts and overrides last-good fallback after explicit source withdrawal.

## Hexagonal layering

```text
Delivery → Application → Domain → Ports ← Adapters
```

Dependencies point inward only. No outer layer may be referenced by an inner
layer.

## Layer responsibilities

### Delivery (apps/server, apps/cli, apps/web)

- HTTP handlers (Axum), CLI commands (Clap), web UI (Dioxus/React), public
  subscription endpoints.
- Dispatches typed requests to application commands/queries.
- Contains no business rules. No cross-repository hand-stitched transactions.

The Delivery layer exposes two distinct HTTP surfaces in `apps/server`:

- **REST admin surface** (`/api/v1/*`): cookie-authenticated, `AdminUser`
  guarded, returns JSON DTOs. Handlers map to application commands/queries.
  Documented in the OpenAPI spec.
- **Public subscription delivery surface** (`/sub/{token}[/{profile}]`,
  `/s/{code}`): path-token authenticated (no cookie), returns generated
  subscription content with profile-specific `Content-Type` and delivery
  headers (`ETag`, `Last-Modified`, `subscription-userinfo`,
  `Cache-Control: private, no-cache`). The delivery handler is a thin adapter:
  it resolves the token, delegates enforcement and generation to Application
  commands, and contains no business rules or cross-repository transactions.
  Not bound to OpenAPI security schemes (uses path tokens, not cookie auth).

DTOs and `ToSchema` derives for both surfaces live in `deve-sub-contract`
per ADR-0004. Path, method, and status definitions live in `apps/server`.

### Application (crates/application)

- Commands (mutate state), queries (read state), jobs, event handlers.
- Orchestrates domain services and ports.
- Does not execute SQL directly. Calls port interfaces.

### Domain (crates/domain)

- Canonical node model, aggregate invariants, domain events.
- Defines port traits. No I/O, no framework types, no database access.
- Depends only on `deve-sub-kernel`.

### Ports (defined in domain/application)

- Interfaces for storage, HTTP fetching, GeoIP, probe, file I/O, release
  update, notification.
- Adapters implement these interfaces.

### Adapters (crates/storage-sqlite, crates/adapters, etc.)

- Implement port traits.
- Contain SQL, HTTP clients, file system access, external service bindings.
- No business rules. No domain logic.

## Inter-module communication

- Modules communicate via public Service, Command, Query, or Domain Event.
- No module reaches into another module's internals.
- No circular dependencies.
- Cross-module calls must appear in this contract or a named more-specific
  contract.

## Forbidden patterns

- API handler that crosses multiple repositories in a hand-stitched transaction
  (constraint #6).
- UI component that parses nodes, generates subscriptions, or judges
  compatibility (constraint #4).
- UI direct database access (constraint #5).
- Generic "universal repository" without business meaning.
- Per-table service without business meaning.
- Full event sourcing (use state tables, audit logs, outbox instead).

## Crate dependency graph

```text
apps/server, apps/cli, apps/web
    ↓
deve-sub-application
    ↓
deve-sub-domain
    ↓
deve-sub-kernel

deve-sub-contract ← shared across delivery and application
deve-sub-protocol, deve-sub-emitter → deve-sub-domain
deve-sub-storage-sqlite, deve-sub-adapters → port traits in domain/application
```

## Authority

- Architecture: `docs/plan/03-architecture.md`
- Workspace layout: `docs/plan/04-workspace-layout.md`
- API boundary: ADR-0001, ADR-0004

### Production dependency checks

The architecture gate classifies every workspace package. Normal,
target-specific and build dependencies obey the same boundary; dev-only
integration-test dependencies may use real adapters. New packages require an
explicit classification. The permitted local dependencies are:

| Package family (`deve-sub-*`) | Permitted production dependencies |
|---|---|
| kernel, contract, ci tooling | No other workspace package |
| domain, security, observability | kernel |
| protocol, emitter, compatibility | kernel, domain |
| application | kernel, contract, domain, security, protocol, emitter, compatibility |
| storage-sqlite, inmemory, adapters | kernel, domain, application, security |
| server | kernel, contract, domain, application, security, compatibility, web |
| web | contract |
| cli | All production packages; never CI tooling |

Domain/value/codec/application packages cannot depend on SQLx, Axum, Reqwest or
Dioxus. Kernel, contract and domain also exclude Tokio. Web cannot depend on
server I/O frameworks; server cannot depend on SQLx or Reqwest. Library errors
remain structured with `thiserror`; `anyhow` is confined to CLI composition
and repository tool entry points. These are dependency checks, not proof that
every function respects its layer; source review remains necessary.

Verification: `python3 scripts/check_architecture.py` checks all tracked and
untracked Rust source, including tooling; `python3 scripts/tests/test_architecture.py`
injects forbidden edges, target/build bypasses and valid integration-test edges.

## HTTP state capabilities

The root server state is a composition container. Route extractors use
`FromRef<AppState>` projections for authentication, users, sources, nodes,
templates, subscriptions, public delivery, probes, dashboards, audit and
health. Each projection exposes only the Ports/services used by that route
family; it cannot be converted back into root state. Authentication guards
use the auth projection. Registration/wiring may mention root state; route
handlers may not extract it. Multi-repository business transactions remain
Application commands backed by atomic storage operations.

Production `apps/server` must not depend on concrete SQLite or general adapter
crates; integration tests may use dev-dependencies. CLI remains the sole
production composition root. Contract DTOs are shared with Web using serde;
OpenAPI derives are enabled by the `openapi` feature only in API delivery.

### Probe traffic commit boundary

`ProbeSourceRepository::commit_sync` owns one transaction: compare the source
revision, persist its new counter/status, insert the traffic delta batch and
its lifetime/daily projections. Delivery dispatches `sync_probe_traffic` and
cannot assemble this transaction. Revision conflict maps to HTTP 409.

### Authentication and credential response boundary

Login and 2FA use the canonical transport peer IP unless explicitly configured
to trust validated proxy headers. Saturated password verification returns 429.
All /api/v1 responses and HTML are no-store; responses also set nosniff,
X-Frame-Options: DENY, frame-ancestors 'none', and no-referrer. Public delivery
keeps private/no-cache and ETag support. Subscription short-code probe counters
are separate from login counters; malformed secret paths are redacted before
tracing. Exact operational limits belong to the M2 and M6 blueprints.

### Node organization boundary

`GET /tags` returns the complete user-authored tag catalog, including tags with
no node assignments. Web categories render this catalog independently of node
pagination; their counts describe loaded node membership, not server totals.
`GET /nodes/{id}/override` returns the complete editable override state.
`PATCH /tags/{id}` replaces the tag's name/color while retaining its identity.
`POST /nodes/batch-tags` accepts `mode: replace | add | remove` (default replace).
Mutation modes are evaluated against current stored memberships atomically;
Delivery never reads and rewrites sets to emulate add/remove. Domain graph
validation uses the same protected snapshot as chain persistence. Invalid or
missing references leave the whole batch unchanged.
