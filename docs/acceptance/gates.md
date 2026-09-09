# Acceptance Gates

## Scope

This document defines the PR-gate command set and evidence-state rules for
Deve Sub. Every PR or commit must run the applicable gates and report real
output or honest not-run state.

## Evidence states

- **pass**: the gate ran and produced successful output.
- **fail**: the gate ran and produced failing output.
- **planned**: the acceptance case is registered but not yet implemented.
  `planned` is not `pass`.
- **not-run**: the gate was not executed for this slice. Report honestly why.
- **blocked**: the gate cannot run due to an external dependency. Report the
  blocker.

Only `pass` counts toward completion. `planned`, `not-run`, and `blocked` are
non-pass states.

## Rust gates

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo test --locked --all-features --doc
```

## Docs gates

- Mermaid syntax: diagrams in `docs/data-model/` are manually reviewed until
  a validator is wired (deferred — no automated gate yet).
- `matrix.yaml` schema: `tests/acceptance/matrix.yaml` must parse as valid
  YAML with the required fields per case.
- `matrix.tsv` consistency: `docs/acceptance/matrix.tsv` must have the same
  case IDs as `matrix.yaml`.
- Coverage-matrix tokens: `docs/coverage-matrix.md` tokens must match
  `matrix.tsv` IDs.

## Acceptance gates

Acceptance cases are registered in `tests/acceptance/matrix.yaml` and
summarized in `docs/acceptance/matrix.tsv`. Each P0 feature must map to at
least one acceptance case ID (constraint #14).

Not all acceptance cases are executable in every milestone. Report the
applicable subset per slice and record the rest as `planned`.

## OpenAPI gate (when API crate exists)

```bash
# Export OpenAPI spec from code
cargo run --locked -p deve-sub-cli -- openapi > docs/openapi/openapi.json
# Verify the spec is up to date
git diff --exit-code docs/openapi/openapi.json
```

Hand-maintaining `docs/openapi/openapi.json` is forbidden (ADR-0004).

## Authority

- Constitution: `docs/plan/00-engineering-constitution.md`
- Work loop: `AGENTS.md`
- Matrix: `tests/acceptance/matrix.yaml`, `docs/acceptance/matrix.tsv`

## Engineering resource and architecture gates

- CI topology and shadow selection: `plan/14-ci-verification.md`; evidence
  format: `contracts/ci-evidence.md`. Run
  `python3 -m unittest discover -s scripts/ci/tests` and
  `python3 scripts/ci/plan.py`. A shadow proposal never skips the full baseline
  or promotes registered matrix evidence into current execution results.
  Rust matrix jobs use `scripts/ci/run_shard.py`; the final gate also checks
  the complete current-source/current-attempt receipt set. Missing or cancelled
  execution evidence fails the gate even if other jobs succeeded.

- `python3 scripts/check_architecture.py`: Cargo boundaries, scoped HTTP state,
  optional OpenAPI dependency, source fuse and immutable Action references.
  Reviewed legacy source-size exceptions live only in
  `scripts/architecture-exceptions.json`; they cannot grow silently.
- Native installer regression: `python3 scripts/tests/test_install.py --binary
  target/debug/deve-sub --web-dir apps/web/dist` requires bubblewrap and built
  assets. It isolates filesystem/PID state, uses real CLI/database/Web paths
  and simulates systemd/account management; it is not DEPLOY-002 VM evidence.
- `cargo deny --locked check`: advisory, license and registry policy. Exact
  advisory exceptions and their rationale live in `deny.toml`.
- `python3 scripts/install-validators.py /tmp/deve-sub-validators`: checksum-
  verified compatibility tools; run the existing ignored validator tests with
  that directory on PATH.
- `python3 scripts/perf/soak.py --binary target/release/deve-sub --seconds 1800
  --require-telemetry --output /tmp/deve-sub-soak.json`: real application
  workload and RSS/FD/WAL/task envelopes. CI uses 90 seconds; short runs are
  accelerated regression evidence and do not establish years-long reliability.
