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
| M4 | Sources and Node Pool | done | M3 | Source CRUD, snapshot, refresh, diff, node pool, override |
| M5 | Generator and V3 Template | done | M3, M4 | Node selection, proxy groups, V3 template, generation, cache |
| M6 | Subscription Distribution | done | M5 | Profile URL, short code, temp link, ETag, auth, traffic |
| M7 | Probes and Detection | done | M6 | Nezha, DStatus, Komari, TCP, QUIC, runner, dashboard |
| M8 | Deployment and Hardening | planned | M7 | Install script, self-update, backup, SSRF, perf, multi-arch |
| M9 | Protocol and Output Expansion | done | M3, M4, M5, M6 | WireGuard, AnyTLS, Snell, ShadowTLS, xhttp, JSON profile |
| M10 | Observability and Audit | done | M6, M7, M2 | Traffic history charts, audit log query API |
| M11 | Archive and Snapshot | done | M6, M7, M10 | `deve-sub backup`, `deve-sub restore`, snapshot format |

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

M4 (Sources and Node Pool) is complete — source CRUD, refresh engine (SSRF
guard, DNS pinning, redirect re-check, body size limit, timeout, gzip/deflate/
brotli/zstd decompression, ETag), node pool (dedup, override, manual region,
GeoIP, batch ops), source filter rules (two-phase protocol + region filter),
concurrent refresh scheduler (semaphore-bounded), zero-node guard, 369 tests.
SRC-001–014, NODE-001/003–011, SEC-001–005 all pass.

M5 (Generator and V3 Template) is complete — V3 subscription template
aggregate (versioned, declarative YAML with `apiVersion: deve-sub.io/v1`),
proxy group model (seven types, chain graph with DFS cycle detection),
node selection (dynamic filters / fixed node IDs, quick-group by
region/protocol/tag, drag-sort), compatibility matrix
(`deve-sub-compatibility` crate), container emitters (mihomo, sing-box,
xray, v2ray, shadowrocket, uri_list), generation pipeline with strict mode
(fail on incompatible) and lenient mode (exclude with report, constraint
#7), generation cache with atomic publish (constraint #19: preserve last
successful on failure), preview endpoint with consistency (preview ==
published). 453 tests pass; GEN-001–016 all pass.

M6 (Subscription Distribution) is complete — profile URL delivery with
token-based auth, short codes, temp links, ETag/conditional GET, traffic quota
enforcement (upload/download/expiry), subscription token rotation, concurrent
generation with atomic cache publish. OUT-008 through OUT-014 all pass.

M7 (Probes and Detection) is complete — probe domain model (ProbeSource,
LatencyRecord, ProbeRun), latency probe runner (TCP connect, QUIC handshake,
real proxy) with semaphore-bounded concurrency and safe shutdown, node chain
proxy with cycle detection, external traffic probe adapters (Nezha, DStatus,
Komari), probe source failure handling (preserve stale stats), multi-source
traffic aggregation with dashboard traceability. PROBE-001 through PROBE-005
all pass; NODE-012 through NODE-018 all pass.

## Next milestones

M8 (Deployment and Hardening) — install script, self-update, backup, SSRF
hardening, performance, multi-arch images. Blueprint:
`docs/plan/milestones/M8-deployment-and-hardening.md`.

M9 (Protocol and Output Expansion) — WireGuard, AnyTLS, Snell, ShadowTLS typed
configs + URI/container parsers/emitters, xhttp transport, JSON output
profile. Blueprint: `docs/plan/milestones/M9-protocol-output-expansion.md`.

M10 (Observability and Audit) — traffic history charts (daily snapshots +
background aggregation job), audit log query API (infrastructure + wiring).
Blueprint: `docs/plan/milestones/M10-observability-and-audit.md`.

M11 (Archive and Snapshot) — `deve-sub backup` and `deve-sub restore` CLI,
versioned backup format, restore-verify path. Low priority. Blueprint:
`docs/plan/milestones/M11-archive-and-snapshot.md`.

## M5 review follow-ups (M6 backlog)

The per-module M5 review surfaced four non-blocking findings deferred to M6.
None weaken the M5 acceptance cases; each is tracked here so M6 closes it.

| ID | Severity | Finding | Owner area | Action | Status |
|---|---|---|---|---|---|
| F5.1 | medium | `crates/deve-sub-application/src/template/selection.rs` is 780 lines (prod 294 + test 486), exceeding the ~500-line hard fuse. | application/template | Split the `#[cfg(test)]` module into a sibling `selection_tests.rs` (or `tests/` within the crate) so production source stays under the fuse. | **done** — tests extracted to `selection_tests.rs` via `#[path]`; prod now 298 lines. |
| F8.1 | medium | `apps/server/src/templates.rs` is 736 lines, exceeding the ~500-line hard fuse. | server/templates | Split DTO mappers, route handlers, and the error mapper into submodules under `apps/server/src/templates/`. | **done** — split into `templates/{mod,crud,resolve,mappers,error}.rs`; largest file 353 lines. |
| F8.3 | medium | `RollbackRequest` and `CompatibilityQuery` are defined in the server crate, but ADR-0004 / `docs/contracts/module-boundaries.md` assign DTOs to the contract crate. | contract/template | Move both DTOs to `crates/deve-sub-contract/src/template.rs`, re-export from server. | **done** — both DTOs already in `deve-sub-contract/src/template.rs` (commit `eaf796e`); server imports from `deve_sub_contract`. No remaining local definitions. |
| F1.1 | low | `docs/plan/00-engineering-constitution.md` §252 says "rollback, verify rollback" but migrations are forward-only (no down migrations), so the wording is plan drift. | plan/constitution | Amend §252 to "apply, verify forward migration; for rollback, restore from backup and re-run migrations" or equivalent forward-only phrasing. | **done** — constitution no longer contains §252 (refactored to 165 lines). The surviving drift was `M5-generator-and-v3-template.md:255`; amended to forward-only backup-restore phrasing matching `13-storage.md`, M4, and M6. |

F4.1 (mihomo manual YAML concatenation) and F5.3 (double DB fetch) and F5.5
(update_template two-call transaction gap) remain low-priority observations
not requiring M6 action; they are noted here for completeness and may be
revisited if touched by other work.

## Authority

- Milestone blueprints: `docs/plan/milestones/`
- Work policy: `module-work-policy.md`
- Agent execution constraints: `docs/plan/00-engineering-constitution.md`
