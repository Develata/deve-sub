# Development Guide

## Prerequisites

- Rust 1.97.1 (pinned by `rust-toolchain.toml`).
- Git.
- SQLite (for local development; the database file is created automatically).
- Node.js 24 (for Playwright E2E and frontend build, when applicable).

## Local commands

```bash
# Check
cargo check --all-targets

# Format
cargo fmt --all -- --check

# Lint
cargo clippy --all-targets -- -D warnings

# Test
cargo test --all-targets
cargo test --doc

# Run a specific package
cargo test -p deve-sub-domain
cargo test -p deve-sub-kernel
```

## CI parity

The local commands above mirror the CI workflow in
`.github/workflows/ci.yml`. Run them before pushing.

## Environment

Copy `.env.example` to `.env` and adjust as needed. The default SQLite database
is created at `data/deve-sub.db` on first run.

### Docker builds behind a TLS inspection proxy

The source Dockerfile accepts an optional BuildKit `build_ca` secret containing
an operator-approved PEM bundle with the normal public roots and the proxy CA:

```bash
docker build --secret id=build_ca,src=/absolute/path/to/combined-ca.pem -t deve-sub:local .
```

Rustup, Cargo and frontend build downloads use that bundle only during their
build steps. Dioxus CLI enables Reqwest's native roots feature so its tool
downloads can also read the bundle. TLS verification stays enabled. The secret
is not copied into image layers, and the runtime image's trust store is
unchanged. Without this input, tools use their normal trust settings. Downloads
without a trusted certificate chain fail; there is no unverified TLS fallback.
BuildKit does not invalidate
cached steps when secret contents change, so use `--no-cache` when checking a
changed bundle. This setting does not override network access policy.

## Database

Migrations live in `migrations/`. The first real migration arrives with M1
(infrastructure). Until then, the placeholder migration exists to track the
directory.

```bash
# Apply migrations (when sqlx-cli is installed and M1 lands)
sqlx migrate run --database-url sqlite:data/deve-sub.db
```

## Spike

UI spike code lives in `spikes/dioxus-ui/` and is excluded from the workspace.
It is not part of the production build. See ADR-0001 and
`docs/plan/milestones/M0-tech-spike.md`.

## Authority

- CI workflow: `.github/workflows/ci.yml`
- Constitution: `docs/plan/00-engineering-constitution.md`
- Storage: `docs/plan/13-storage.md`
