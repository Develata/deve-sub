# Contributing

Deve Sub uses a docs-as-code structure with layered authority. Read
[`AGENTS.md`](AGENTS.md) first, then [`docs/README.md`](docs/README.md) and the
matching documents in [`docs/coverage-matrix.md`](docs/coverage-matrix.md).

## Workflow

1. Bind the slice to a milestone, acceptance case, or ADR.
2. Follow the mandatory work loop in `AGENTS.md` (read plan -> contracts ->
   implement -> gate -> review -> commit).
3. Keep one semantic intent per commit and stage exact paths only (`git add -A`
   is forbidden).
4. Run the owning gates before pushing and include real output or honest
   not-run state.

## Branch policy

Before the first tagged release:

- Work directly on `main`; atomic commits; direct push to remote `main` is
  allowed.
- Run `git status` and `git pull --ff-only` at the start of every work round.
- Force push, rewriting remote `main` history, and dropping unrelated user
  changes are forbidden.

After the first tagged release:

- Switch to `feat/<topic>`, `fix/<topic>`, `docs/<topic>`, `chore/<topic>` or
  disposable `spike/<topic>` branches.
- Pull requests, required CI checks, and `main` branch protection apply; direct
  push to `main` is forbidden.

## Commit style

Concise conventional-style commits, one semantic intent each:

```text
docs: align storage authority layers
feat: add canonical node model
test: cover VLESS Reality round-trip
```

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --locked --all-targets --all-features -- -D warnings`
- `cargo test --locked --all-targets --all-features`
- `cargo test --locked --all-features --doc`
- Docs gates: Mermaid syntax, `matrix.yaml` schema, coverage-matrix token
  consistency (see `.github/workflows/ci.yml`).
