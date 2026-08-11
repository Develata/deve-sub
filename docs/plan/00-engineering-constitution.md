# 00 — Engineering Constitution

## Purpose

This document owns the long-lived engineering principles for Deve Sub. All
code and documentation in this repository must comply with it.

## Priority order

When priorities conflict, lower-priority work must not weaken higher-priority
work:

1. Correctness / functional truth
2. Safety / reversibility / data integrity
3. Usability / user-visible workflow
4. Public contract stability — preserve existing entry points, paths,
   configuration shapes, public APIs, deployment assumptions, and
   client-visible semantics unless the task explicitly changes them
5. Maintainability / diagnosability
6. Performance
7. Memory footprint
8. Disk footprint

Default conservative: choose the stable, simple, reversible path unless Deve
explicitly chooses the aggressive path.

## Rust discipline

### Safety

`unsafe_code = "forbid"` across the workspace. If `unsafe` is ever needed,
every block must include a `SAFETY` comment explaining the invariant upheld.

### Clippy

`dbg_macro = "deny"`, `todo = "deny"`, `unwrap_used = "deny"`,
`expect_used = "deny"`.

`expect_used` is denied in non-test code. Library crates use
`#![cfg_attr(test, allow(clippy::expect_used))]` at the crate root so that
`#[cfg(test)]` unit-test modules are exempt. Integration test files use
`#![allow(clippy::expect_used)]`. A non-test `.expect()` requires a targeted
`#[allow(clippy::expect_used)]` plus a WHY comment justifying infallibility.

### Error layer

All library crates (domain, application, protocol, emitter, adapter) use
`thiserror` for structured errors.

`anyhow` is limited to:

- server/cli binary entry points;
- dependency injection and composition roots;
- top-level CLI command execution;
- startup, configuration loading, and migration orchestration.

`anyhow` must not enter Ports, Domain Services, Application Services, or any
public library API.

### Comments

No comments that restate code surface behavior. Public APIs, public types,
important traits, and core domain models must use rustdoc.

WHY comments are required for:

- safety invariants;
- protocol compatibility and field mapping;
- data consistency constraints;
- concurrency, lock order, and transaction boundaries;
- counter-intuitive implementation choices;
- temporary compatibility shims with removal conditions;
- regression guards.

All `unsafe` blocks must include a `SAFETY` comment.

Reference design decisions by stable ADR number (e.g. `/// See ADR-0003.`),
not by `plan_ref` comments.

### File size fuse

- Files over ~250 lines are soft architecture warnings; review for cohesion,
  duplication, and hidden coupling.
- Hand-written source files over ~500 lines are hard fuse violations unless
  explicitly justified.
- Tests may exceed the soft threshold when keeping scenario context together
  improves readability.

## Naming

- Cargo packages: `deve-sub-*` (e.g. `deve-sub-domain`, `deve-sub-server`).
- Crate imports: `deve_sub_*` (e.g. `deve_sub_domain`).
- Binary: `deve-sub` with subcommands (`serve`, `doctor`, `health`, `migrate`,
  `source`, `node`, `subscription`, `user`, `backup`, `restore`, `update`).
  <!-- `health` sanctioned per ADR-0006 (Docker Base Image and Internalized
  Healthcheck). -->
- Product name: "Deve Sub". Centralized in configuration; no hardcoded
  scattering.
- V3 template namespace: `deve-sub.io/v1`.

## License

MIT License. Copyright (c) 2026 Develata. No per-file license header is
required. Third-party code, icons, fonts, GeoIP data, test fixtures, and other
resources must retain and follow their own licenses.

## Git strategy

### Before the first tagged release

- Work directly on `main`; atomic commits; direct push to remote `main` is
  allowed.
- Run `git status` and `git pull --ff-only` at the start of every work round.
- Run `cargo fmt --check`, `cargo check`, `cargo test`, and other configured
  CI checks before pushing.
- Forbidden: force push, rewriting remote `main` history, dropping unrelated
  user changes, unconfirmed `reset --hard` / `rebase` / branch deletion /
  remote changes, and merging unrelated stages into one large commit.

### After the first tagged release

- Switch to `feat/`, `fix/`, `docs/`, `chore/`, or disposable `spike/`
  branches.
- Pull requests, required CI checks, and `main` branch protection apply; direct
  push to `main` is forbidden.

Do not push tags, create releases, or change GitHub visibility without
explicit Deve authorization.

## Agent execution constraints

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

## Data and security

- No secrets, real personal data, `.env`, tokens, private keys, or real proxy
  node credentials in commits, fixtures, logs, or screenshots. Fixtures use
  reserved test identifiers.
- Sensitive fields (subscription URLs, cookies, custom headers) are encrypted
  with XChaCha20-Poly1305; the master key comes from a file or secret mount.
- Subscription tokens are CSPRNG-generated, stored as HMAC-SHA256 digests, and
  redacted in logs.
- Entity identifiers use strong-typed ULIDs, server-monotonically generated.
  The database also stores `created_at`. Cursor pagination uses opaque
  `(created_at, id)` encoding; clients must not depend on the internal format.
  ULIDs identify entities only; they must not be used for login sessions,
  subscription tokens, recovery codes, or any other secret.
