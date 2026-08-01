# Contributing Guide

## Start here

1. [`AGENTS.md`](../../AGENTS.md) — repository governance and work loop.
2. [`docs/plan/00-engineering-constitution.md`](../plan/00-engineering-constitution.md)
3. [`docs/plan/01-terminology.md`](../plan/01-terminology.md)
4. [`docs/tasks/module-work-policy.md`](../tasks/module-work-policy.md)
5. [`docs/tasks/execution-roadmap.md`](../tasks/execution-roadmap.md)

## Workflow

1. Bind the slice to a milestone, acceptance case, or ADR.
2. Follow the mandatory work loop (read plan → contracts → implement → gate →
   review → commit).
3. Keep one semantic intent per commit. Stage exact paths only.
4. Run the owning gates before pushing and include real output or honest
   not-run state.

## Branch policy

Before the first tagged release: work directly on `main`, atomic commits,
direct push allowed.

After the first tagged release: `feat/`, `fix/`, `docs/`, `chore/`, `spike/`
branches with pull requests and `main` branch protection.

See `docs/plan/00-engineering-constitution.md` §"Git strategy" for full rules.

## Commit style

```text
docs: align storage authority layers
feat: add canonical node model
test: cover VLESS Reality round-trip
fix: preserve three-state TLS on emit
```

## Review

- At most three review subagents concurrently.
- The main agent independently verifies every finding.
- Scope: high cohesion, low coupling, boundary drift, file size, failure
  paths, verification coverage, thin-shell violations.

## Authority

This guide defers to `AGENTS.md`, `docs/AGENTS.md`, and `docs/plan/`. It does
not redefine governance or architecture.
