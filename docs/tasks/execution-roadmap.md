# Execution Roadmap

## Scope

This document maps the eight milestones from the spec to the current delivery
order. It schedules approved contracts; it cannot override plans.

## Milestones

| Milestone | Name | Status | Dependency | Vertical slice |
|---|---|---|---|---|
| M0 | Tech Spike | planned | none | UI spike gate, SQLite spike, Docker build |
| M1 | Infrastructure | planned | M0 | `deve-sub serve` with health, API skeleton, DB, Docker |
| M2 | Auth and Users | planned | M1 | Login, RBAC, session, 2FA, user management |
| M3 | Protocol Engine | planned | M1 | Canonical model, P0 parsers, P0 emitters, golden + fuzz |
| M4 | Sources and Node Pool | planned | M3 | Source CRUD, snapshot, refresh, diff, node pool, override |
| M5 | Generator and V3 Template | planned | M3, M4 | Node selection, proxy groups, V3 template, generation, cache |
| M6 | Subscription Distribution | planned | M5 | Profile URL, short code, temp link, ETag, auth, traffic |
| M7 | Probes and Detection | planned | M6 | Nezha, DStatus, Komari, TCP, QUIC, runner, dashboard |
| M8 | Deployment and Hardening | planned | M7 | Install script, self-update, backup, SSRF, perf, multi-arch |

## Phase 1

Phase 1 is split into:

- **1A — Architecture closure**: ADR, ER, canonical node model, acceptance
  matrix, CI, governance docs. No runnable feature path; smoke is not
  applicable.
- **1B — Dioxus UI Spike**: the M0 UI spike gate. See
  `docs/plan/milestones/M0-tech-spike.md`.

1A must be reviewed and closed before 1B begins.

## Current status

Phase 1A is complete (architecture closure: ADRs, ER, canonical node model,
acceptance matrix, CI, governance docs — 20 commits, reviewed and closed).
Phase 1B (M0 Tech Spike) is next.

## Authority

- Milestone blueprints: `docs/plan/milestones/`
- Work policy: `module-work-policy.md`
- Agent execution constraints: `docs/plan/00-engineering-constitution.md`
