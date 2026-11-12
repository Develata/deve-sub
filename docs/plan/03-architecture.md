# 03 — Architecture

## Scope

This chapter defines the system's layering, module rules, and the mapping from
UI operations to application use cases.

## Hexagonal layering

```text
┌──────────────── Delivery ────────────────┐
│ Dioxus Web │ REST API │ CLI │ Public Sub │
└──────────────────┬───────────────────────┘
                   │
┌──────────────── Application ─────────────┐
│ Commands │ Queries │ Jobs │ Event Handler │
└──────────────────┬───────────────────────┘
                   │
┌────────────────── Domain ────────────────┐
│ Source │ Node │ Template │ Subscription   │
│ Identity │ Probe │ Traffic │ Compatibility│
└──────────────────┬───────────────────────┘
                   │ Ports
┌──────────────── Adapters ────────────────┐
│ SQLite │ HTTP │ GeoIP │ Probe │ Files     │
│ Release Updater │ Notification │ Test Core│
└──────────────────────────────────────────┘
```

Dependencies point inward only. Delivery depends on Application. Application
depends on Domain. Domain defines Ports. Adapters implement Ports.

## Lightweight CQRS

Commands mutate state; queries read state. One UI operation maps to one
application command or query. API handlers dispatch to commands/queries but
contain no business rules.

## API boundary

The typed REST `/api/v1` surface is the API boundary. P0 core business must
not depend on Dioxus Server Functions. See ADR-0001 and ADR-0004.

## Module rules

Each business module contains at minimum:

```text
domain.rs
commands.rs
queries.rs
service.rs
ports.rs
events.rs
errors.rs
dto.rs
```

1. Domain does not depend on Axum, Dioxus, or SQLx.
2. Application does not execute SQL directly.
3. API handlers contain no business rules.
4. The frontend does not depend on database models directly.
5. Modules communicate via public Service, Command, Query, or Domain Event.
6. No circular dependencies.
7. No global mutable state.
8. No per-table Service without business meaning.
9. No generic "universal repository."
10. No full event sourcing. Use state tables, audit logs, and a persistent
    outbox.

## UI button to use-case mapping

One UI operation maps to one application use case. Example:

```text
"立即更新订阅源"
    ↓
POST /api/v1/sources/{id}/refresh
    ↓
RefreshSourceCommand
    ↓
SourceRefreshService
    ↓
SourceFetcher Port
    ↓
NodeReconciler
    ↓
SourceRefreshed Event
    ↓
SubscriptionCacheInvalidator
```

Buttons must not trigger scattered database modifications directly.

## Thin frontend

The frontend renders UI, collects intent, and dispatches typed requests. It
must not perform node parsing, protocol conversion, subscription generation,
compatibility judgment, security-field correction, subscription source merging,
core YAML generation, or permission logic. See `docs/plan/01-terminology.md`
§"Thin frontend".

Theme presentation (UI-003/004/005/006) uses shared semantic colors for the
page canvas, surfaces, inputs, borders, primary text, secondary text and accent.
Light, Dark, Warm and Violet must visibly affect the complete application shell
and dialogs. Theme and custom-accent preferences persist independently.
Browser acceptance checks computed colors and rendered screenshots on actual
pages after reload; an HTML class or storage value alone is insufficient proof.

The public health response supplies the configured product name. One reactive
Web branding state supplies the shell, authentication screens and document
title. A browser-local display-name preference may override it; clearing that
preference restores the server name. This presentation preference does not
change server configuration or the authentication issuer. UI-006 verifies the
visible name after saving, navigation and reload.

## Authority

- Typed module boundaries: `docs/contracts/module-boundaries.md`
- API/OpenAPI toolchain: ADR-0004
- Frontend mode: ADR-0001

## Verification

- Architecture drift is detected by review and contract checks.
- Each UI operation trace is verifiable through the API surface to the
  application command/query.
