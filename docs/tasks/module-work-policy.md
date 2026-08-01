# Module Work Policy

## Scope

This document defines how work is sliced, owned, reviewed, and committed in
Deve Sub. It schedules work but does not redefine product topology or
invariants.

## Milestone slicing

Deve Sub is a modular monolith. Work is organized by milestone (see
`execution-roadmap.md`). Each milestone delivers a runnable vertical slice
(constraint #12).

- A milestone is split into multiple commits, each with one semantic intent.
- A commit may span docs, contracts, and code as long as it serves one intent.
- Stage exact files only. `git add -A` is forbidden.

## Before a nontrivial slice

1. Bind the slice to a milestone, acceptance case, or ADR.
2. Name one owner and one reviewer.
3. Declare touched directories and non-goals.
4. Follow the mandatory work loop in `AGENTS.md`.

## Commit discipline

- One semantic intent per commit.
- Stage exact paths only.
- Include real validation output or honest not-run state.
- Run `cargo fmt --check`, `cargo check`, `cargo test`, and applicable docs
  gates before pushing.

## Review

- Run review with at most three review subagents when available.
- The main agent independently verifies every finding, fixes all accepted
  blockers, and closes every review lane.
- Review scope: high cohesion, low coupling, boundary drift, file size, failure
  paths, verification coverage, thin-shell violations.

## Git strategy

Before the first tagged release:

- Work directly on `main`; atomic commits; direct push to remote `main` is
  allowed.
- Run `git status` and `git pull --ff-only` at the start of every work round.

After the first tagged release:

- Switch to `feat/`, `fix/`, `docs/`, `chore/`, or `spike/` branches.
- Pull requests, required CI checks, and `main` branch protection apply.

See `docs/plan/00-engineering-constitution.md` §"Git strategy" for the full
rules.

## Authority

- Work loop and slice completion: `AGENTS.md`
- Git strategy: `docs/plan/00-engineering-constitution.md`
- Execution order: `execution-roadmap.md`
