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
