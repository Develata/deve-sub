# 04 — Workspace Layout

## Scope

This chapter defines the Cargo workspace structure, crate naming, and
dependency direction. The target is 12–16 crates; one feature per crate is
avoided to keep compile times controlled.

## Layout

```text
apps/
├── server/                 # HTTP, Web UI, public subscription
├── cli/                    # CLI and headless management
└── web/                    # Dioxus Web

crates/
├── kernel/                 # ID, time, pagination, common errors
├── contract/               # API DTO, event DTO, client capabilities
├── domain/                 # domain model
├── application/            # commands, queries, use cases
├── protocol/               # input parsing and canonical model
├── emitter/                # target format output
├── compatibility/          # client capability profiles
├── storage-sqlite/         # SQLite adapter
├── storage-postgres/       # later version
├── adapters/               # HTTP, GeoIP, probe, files
├── scheduler/              # job scheduling
├── security/               # auth, crypto, SSRF
├── observability/          # tracing, metrics
└── testkit/                # test helpers

spikes/                     # excluded from workspace (UI spike, etc.)
frontend-assets/
migrations/
fixtures/
docs/
scripts/
deploy/
```

## Crate naming

- Cargo packages: `deve-sub-*` (e.g. `deve-sub-domain`, `deve-sub-server`).
- Crate imports: `deve_sub_*` (e.g. `deve_sub_domain`).
- Binary: `deve-sub`.

## Dependency direction

```text
apps/server, apps/cli, apps/web
    ↓
crates/application
    ↓
crates/domain
    ↓
crates/kernel
```

`crates/contract` is shared across delivery and application. `crates/protocol`
and `crates/emitter` depend on `crates/domain`. Adapter crates depend on Port
traits defined in `crates/domain` or `crates/application`, not the other way
around.

## Spike exclusion

`spikes/*` are excluded from the workspace (`exclude = ["spikes/*"]` in the
root `Cargo.toml`). Spike code is experimental and not part of the production
build. See ADR-0001 for the frontend spike policy.

## Phase 1 crates

Phase 1 scaffolds three foundational crates only:

- `deve-sub-kernel` — ID, time, pagination, errors.
- `deve-sub-contract` — API DTOs, OpenAPI schemas.
- `deve-sub-domain` — canonical node model.

Remaining crates are added as their milestones begin.

## Authority

- Naming rules: `docs/plan/00-engineering-constitution.md` §"Naming"
- Frontend spike: ADR-0001
