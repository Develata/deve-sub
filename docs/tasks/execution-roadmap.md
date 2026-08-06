# Execution Roadmap

## Scope

This document maps the eight milestones from the spec to the current delivery
order. It schedules approved contracts; it cannot override plans.

## Milestones

| Milestone | Name | Status | Dependency | Vertical slice |
|---|---|---|---|---|
| M0 | Tech Spike | done | none | UI spike gate, SQLite spike, Docker build |
| M1 | Infrastructure | done | M0 | `deve-sub serve` with health, API skeleton, DB, Docker |
| M2 | Auth and Users | done | M1 | Login, RBAC, session, 2FA, user management |
| M3 | Protocol Engine | done | M1 | Canonical model, P0 parsers, P0 emitters, golden + fuzz |
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
Phase 1B (M0 Tech Spike) is complete — Dioxus UI spike (22/22 Playwright
tests pass), SQLite concurrency spike, Docker build spike (design validated).

M1 (Infrastructure) is complete — Axum server with tower-http middleware,
SQLite WAL pool, initial migration (users/sessions/audit_log/outbox),
CLI subcommands (serve/doctor/migrate/config validate/openapi), OpenAPI
spec export, Dockerfile (multi-stage, non-root, healthcheck). Docker image
build verification pending CI (no local Docker daemon).

M2 (Auth and Users) is complete — argon2id password hashing, CSPRNG session
tokens (HMAC-SHA256 digests, HttpOnly SameSite cookies), setup-admin gate,
login/logout with timing side-channel mitigation, RBAC (AdminUser guard),
user CRUD with cursor pagination, disable user + force logout, login rate
limiting (per-username and per-IP), CSRF protection (Origin header validation),
2FA (TOTP RFC 6238, XChaCha20-Poly1305 encrypted secrets, recovery codes,
stateless challenge tokens). 110 tests pass; AUTH-001–008, AUTH-010, SEC-009,
SEC-010 marked pass.

M3 (Protocol Engine) is complete — `deve-sub-protocol` crate with URI
parsers and container format parsers for all 7 P0 protocols (VLESS Reality,
Hysteria2, TUIC v5, NaiveProxy, Shadowsocks, VMess, Trojan), `deve-sub-emitter`
crate with URI emitters, 36 golden tests + 15 fuzz/property tests.
PARSE-001–018 all pass.

M4 (Sources and Node Pool) is next.

## Authority

- Milestone blueprints: `docs/plan/milestones/`
- Work policy: `module-work-policy.md`
- Agent execution constraints: `docs/plan/00-engineering-constitution.md`
