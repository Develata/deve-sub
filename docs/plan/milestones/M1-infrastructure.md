# Milestone 1 — Infrastructure

## Scope

Establish the runtime foundation: workspace, domain boundaries, Axum server,
Dioxus web shell, SQLx, migrations, configuration, logging, OpenAPI, CLI,
Docker, and health checks. M1 delivers a runnable vertical slice: a server
that starts, serves a health endpoint, and renders a placeholder web page.

## Dependency

M0 (tech spike) must be complete. The frontend technology choice is locked
before M1 begins full UI work.

## Vertical slice

```text
deve-sub serve
    → Axum HTTP server on configured bind address
    → /health/live and /health/ready endpoints
    → /api/v1 skeleton with OpenAPI spec served at /docs
    → Dioxus (or React) web shell serving a placeholder page
    → SQLite database with initial migration applied
    → structured tracing logs
    → CLI with serve, doctor, migrate subcommands
    → Docker image buildable and healthy
```

## Deliverables

- Workspace expansion: `deve-sub-application`, `deve-sub-server`, `deve-sub-cli`,
  `deve-sub-storage-sqlite`, `deve-sub-security`, `deve-sub-observability`.
- Axum 0.8 server with tower-http middleware (CORS, compression, request ID,
  tracing).
- SQLx SQLite connection pool with WAL configuration from
  `docs/plan/13-storage.md`.
- Initial migration: schema for users, sessions, audit log, and outbox (real
  schema arrives with M2).
- Configuration loading from file and environment. Product name centralized.
- Tracing subscriber with structured logs.
- OpenAPI: utoipa + utoipa-axum + utoipa-scalar. Spec exported to
  `docs/openapi/openapi.json` in CI. See ADR-0004.
- CLI: `deve-sub serve`, `deve-sub serve --headless`, `deve-sub doctor`,
  `deve-sub migrate`, `deve-sub config validate`.
- Docker: multi-stage build on `debian:trixie-slim`, non-root user, healthcheck via internalized `deve-sub health`, amd64 + arm64. Amended per ADR-0006: trixie-slim replaces bookworm-slim, and the healthcheck probes `/health/live` through the CLI binary instead of a bundled `curl`.
- Health endpoints: `/health/live`, `/health/ready`.

## Authority

- Architecture: `docs/plan/03-architecture.md`
- Workspace layout: `docs/plan/04-workspace-layout.md`
- Storage: `docs/plan/13-storage.md`, ADR-0002
- API/OpenAPI: ADR-0004
- Frontend: ADR-0001

## Verification

- `deve-sub serve` starts and `/health/live` returns 200.
- `deve-sub doctor` checks database, directories, network, and version.
- `deve-sub migrate` applies migrations idempotently.
- OpenAPI spec is generated and exportable.
- Docker image starts and healthcheck passes via internalized `deve-sub health`.
- Acceptance: `CLI-001`, `CLI-005`, `DEPLOY-001`, `DEPLOY-003`, `DEPLOY-004`.
