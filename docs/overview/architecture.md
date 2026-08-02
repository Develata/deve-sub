# Architecture Overview

## Scope

This document is a cross-layer navigation map. It summarizes and links; it does
not own behavior. Authority lives in `docs/plan/` and `docs/contracts/`.

## Layer map

```text
┌──────────────── Delivery ────────────────┐
│ Dioxus Web │ REST API │ CLI │ Public Sub │  → apps/server, apps/cli, apps/web
└──────────────────┬───────────────────────┘
                   │ typed REST /api/v1
┌──────────────── Application ─────────────┐
│ Commands │ Queries │ Jobs │ Event Handler │  → crates/application
└──────────────────┬───────────────────────┘
                   │
┌────────────────── Domain ────────────────┐
│ Source │ Node │ Template │ Subscription   │  → crates/domain
│ Identity │ Probe │ Traffic │ Compatibility│
└──────────────────┬───────────────────────┘
                   │ Ports (traits)
┌──────────────── Adapters ────────────────┐
│ SQLite │ HTTP │ GeoIP │ Probe │ Files     │  → crates/storage-sqlite, crates/adapters
│ Release Updater │ Notification │ Test Core│
└──────────────────────────────────────────┘
```

## Document navigation

| Layer | Plan | Contract | Acceptance |
|---|---|---|---|
| Constitution | `plan/00-engineering-constitution.md` | `AGENTS.md` | `acceptance/gates.md` |
| Terminology | `plan/01-terminology.md` | `contracts/data-models.md` | — |
| Architecture | `plan/03-architecture.md` | `contracts/module-boundaries.md` | — |
| Workspace | `plan/04-workspace-layout.md` | — | — |
| Protocol engine | `plan/05-protocol-engine.md` | `contracts/data-models.md` | `PARSE-*`, `NODE-*` |
| Output profiles | `plan/06-output-profiles.md` | — | `OUT-*` |
| Storage | `plan/13-storage.md` | — | `DEPLOY-*`, `PERF-*` |
| Data model | `data-model/core-er.md` | `contracts/data-models.md` | — |

## Key decisions

| ADR | Decision |
|---|---|
| ADR-0001 | Dioxus Web CSR + typed REST, spike gate |
| ADR-0002 | Storage Port, SQLite-first |
| ADR-0003 | Canonical Node Model, ProtocolKind 15+Unknown |
| ADR-0004 | utoipa + utoipa-axum + utoipa-scalar |
| ADR-0005 | TLS skip_cert_verify three-state |

## Current status

Phase 1A (architecture closure) is in progress. See
`docs/tasks/execution-roadmap.md` for milestone status.
