# AGENTS.md — Deve Sub

## Scope

This file governs all work in this repository. Read the global
`~/.config/opencode/AGENTS.md` first for agent role and communication style,
then this file, then `docs/README.md` and the nearest contract before editing.
Project-level rules override the global file on conflict.

## Governance layers

- Global `~/.config/opencode/AGENTS.md` owns agent role, communication style,
  engineering priorities, and delivery discipline across all projects.
- This root `AGENTS.md` owns repository-wide product topology, technology
  boundaries, collaboration rules, work loop, Git strategy, and delivery
  discipline.
- `docs/plan/` owns the engineering blueprint: constitution, terminology, and
  per-milestone module blueprints.
- `docs/contracts/` owns typed module boundaries, schemas, permissions, and
  interfaces.
- `docs/features/`, `docs/acceptance/`, `docs/tasks/` project behavior, proof,
  and current delivery order; they do not redefine higher authority.
- Place a rule in the narrowest stable owner. Move semantics before deleting
  stale copies; do not maintain peer authorities for one rule.

## Product boundary

- Project: **Deve Sub** — a self-hosted proxy subscription infrastructure
  manager.
- Architecture: modular monolith with hexagonal layering and lightweight CQRS,
  not a microservice collection.
- Core spine: sources and single nodes → fetch/parse/standardize → unified node
  pool → filter/dedup/sort/edit/chain → proxy groups and rules → multi-client
  generation → user authorization, traffic and expiry control → long-term
  subscription URLs.
- Single binary `deve-sub` with subcommands; thin web UI renders server-owned
  state and dispatches typed intent.
- Product name, logo, and site title are centrally configured; no hardcoded
  scattering.

## Source of truth

- `docs/plan/` is the engineering blueprint authority. `docs/contracts/` owns
  typed boundaries. Code is a projection of approved plans and contracts, not
  an independent source of design authority.
- `migrations/` is the physical schema source of truth; `docs/data-model/` is
  the conceptual entity model.
- OpenAPI spec is generated from code via utoipa and exported to
  `docs/openapi/openapi.json`; hand-maintaining the spec is forbidden.
- `Cargo.lock` pins all dependency versions. Production images must not use
  unpinned Git dependencies or `latest` tags.
- Treat code that disagrees with a current plan invariant as implementation
  drift. Align code to plan, or record explicit drift evidence; do not weaken
  the plan merely because code already exists.

## Engineering rules

Apply the global priority order (correctness → safety → usability → contract
stability → maintainability → performance → memory → disk). In this repository,
lower-priority work must not weaken security invariants, protocol fidelity,
data consistency, authority boundaries, or recoverability.

### Architecture constraints

- Hexagonal layering: Delivery (Dioxus Web / REST API / CLI / public
  subscription) → Application (commands, queries, jobs, event handlers) →
  Domain → Ports → Adapters. Dependencies point inward only.
- The frontend is a thin shell: render UI, collect intent, dispatch typed
  requests. No node parsing, protocol conversion, subscription generation,
  compatibility judgment, security-field correction, or permission logic in the
  frontend.
- P0 core business must not depend on Dioxus Server Functions. The API boundary
  is the typed REST `/api/v1` surface.
- API handlers must not contain business rules or cross multiple repositories
  in a hand-stitched transaction. One UI operation maps to one application
  command/query.
- No microservices to solve ordinary module boundary problems. Prefer a
  modular monolith with clear Ports.
- Do not build generic "universal repositories" or per-table services without
  business meaning.
- No full event sourcing. Use state tables, audit logs, and a persistent
  outbox.

### Rust discipline

- `unsafe_code = "forbid"` across the workspace. If `unsafe` is ever needed,
  every block must include a `SAFETY` comment.
- Clippy denies: `dbg_macro`, `todo`, `unwrap_used`.
- Error layer: all library crates (domain, application, protocol, emitter,
  adapter) use `thiserror` for structured errors. `anyhow` is limited to binary
  entry points, composition roots, top-level CLI execution, startup, config
  loading, and migration orchestration. `anyhow` must not enter Ports, Domain
  Services, Application Services, or any public library API.
- Comments: no comments that restate code surface behavior. Public APIs, public
  types, important traits, and core domain models must use rustdoc. WHY
  comments are required for safety invariants, protocol compatibility and field
  mapping, data consistency constraints, concurrency/lock order/transaction
  boundaries, counter-intuitive choices, temporary compatibility shims with
  removal conditions, and regression guards. Reference design decisions by
  stable ADR number (e.g. `/// See ADR-0003.`), not by `plan_ref` comments.
- File size fuse: files over ~250 lines are soft architecture warnings;
  hand-written source files over ~500 lines are hard fuse violations unless
  explicitly justified. Tests may exceed the soft threshold when keeping
  scenario context together improves readability.

### Naming

- Cargo packages: `deve-sub-*` (e.g. `deve-sub-domain`, `deve-sub-server`).
- Crate imports: `deve_sub_*` (e.g. `deve_sub_domain`).
- Binary: `deve-sub` with subcommands (`serve`, `doctor`, `migrate`, `source`,
  `node`, `subscription`, `user`, `backup`, `restore`, `update`).
- Product name: "Deve Sub". Centralized in configuration.
- V3 template namespace: `deve-sub.io/v1`.

### Agent execution constraints

The full list of twenty execution constraints lives in
`docs/plan/00-engineering-constitution.md`. The binding highlights:

1. Phase 1 delivers ADR, ER, canonical node model, and tech spike only.
2. No multiple output converters before the canonical node model is complete.
3. No claiming protocol support without a passing round-trip test.
4. No core generation logic in UI components.
5. No UI direct database access.
6. No API handler cross-repository hand-stitched transactions.
7. No silent dropping of incompatible nodes.
8. No auto-changing certificate verification security semantics.
9. No real node credentials in repo fixtures.
10. No template executing arbitrary scripts.
11. No `latest` image as a production release dependency.
12. Each milestone provides a runnable vertical slice.
13. Each database change has a migration and a recovery test.
14. Each P0 feature maps to an acceptance case ID.
15. Each release generates SBOM, checksums, and signatures.
16. No microservices for ordinary module boundary issues.
17. Prefer modular monolith and clear Ports over abstract frameworks.
18. Compatibility conclusions require client validation or official format.
19. On failure, preserve the last successful subscription version.
20. All async background tasks are observable, cancellable, and safely shut
    down.

### Data and security

- No secrets, real personal data, `.env`, tokens, private keys, or real proxy
  node credentials in commits, fixtures, logs, or screenshots. Fixtures use
  reserved test identifiers.
- Sensitive fields (subscription URLs, cookies, custom headers) are encrypted
  with XChaCha20-Poly1305; the master key comes from a file or secret mount.
- Subscription tokens are CSPRNG-generated, stored as HMAC-SHA256 digests, and
  redacted in logs. ULIDs identify entities only; they are not secrets.

## Repository architecture

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

The full workspace layout (~12-16 crates) lives in
`docs/plan/04-workspace-layout.md`.

## Mandatory work loop

Every work item follows this order:

1. Run `git status` and `git pull --ff-only`.
2. Read `docs/plan/00-engineering-constitution.md`.
3. Read `docs/plan/01-terminology.md`.
4. Read the relevant `docs/plan/` chapters and decide whether the blueprint must
   change before code.
5. Read the corresponding contracts, features, acceptance rows, and tasks, then
   decide which projections must change.
6. Implement the smallest cohesive code or documentation slice only after the
   governing documents are clear.
7. Run a quick gate over every changed **and untracked** file in the slice;
   plain `git diff` is insufficient for new files.
8. Run review with at most three review subagents when available. The main
   agent independently verifies every finding, fixes all accepted blockers, and
   closes every review lane.
9. Run final baseline checks: `cargo fmt --check`, `cargo clippy -D warnings`,
   `cargo test`, doc tests, and docs/acceptance gates.
10. Exercise the real feature path when the slice has a runnable surface;
    otherwise record the smoke as not applicable. Stage exact files and commit.

Code is a projection of `docs/plan/` and contracts. Tests and real smoke are
projections of acceptance rows. A task or implementation convenience cannot
override the plan.

## Slice completion

A slice is done only when:

- the owning plan, contract, coverage, and acceptance projections are current;
- status is reported honestly as implemented, planned, blocked, or not-run;
- changed-file and acceptance gates pass with real output;
- an applicable client/CLI/API/runtime path is exercised, or recorded as not
  applicable;
- review has no unresolved accepted blocker;
- exact staged paths and cached diff are inspected before commit;
- push, merge, release, publication, and visibility changes occur only under
  current authorization.

## Collaboration model

Work is organized by milestone (see `docs/tasks/execution-roadmap.md`). Before
a nontrivial slice:

1. Bind the slice to a milestone, acceptance case, or ADR.
2. Name one owner and one reviewer; declare touched directories and non-goals.
3. Keep one semantic intent per commit. Stage exact files only; `git add -A` is
   forbidden.
4. Include real validation output in the commit message or review packet.

## Git strategy

Before the first tagged release:

- Work directly on `main`; atomic commits; direct push to remote `main` is
  allowed.
- Run `git status` and `git pull --ff-only` at the start of every work round.
- Run `cargo fmt --check`, `cargo check`, `cargo test`, and other configured CI
  checks before pushing.
- Forbidden: force push, rewriting remote `main` history, dropping unrelated
  user changes, unconfirmed `reset --hard` / `rebase` / branch deletion / remote
  changes, and merging unrelated stages into one large commit.

After the first tagged release:

- Switch to `feat/`, `fix/`, `docs/`, `chore/`, or disposable `spike/`
  branches.
- Pull requests, required CI checks, and `main` branch protection apply; direct
  push to `main` is forbidden.

Do not push tags, create releases, or change GitHub visibility without
explicit Deve authorization.

## Public transition guard

This repository may become public. Before that: scrub secrets, verify fixture
data, confirm license compatibility of third-party resources (icons, fonts,
GeoIP data), and pass the public-readiness checklist.
