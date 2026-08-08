# Module Boundaries

## Scope

This contract defines the typed module boundaries, dependency direction, and
inter-module communication rules for Deve Sub.

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
