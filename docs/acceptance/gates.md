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

- Mermaid syntax: diagrams in `docs/data-model/` must render.
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
cargo run -p deve-sub-server -- export-openapi --output docs/openapi/openapi.json
# Verify the spec is up to date
git diff --exit-code docs/openapi/openapi.json
```

Hand-maintaining `docs/openapi/openapi.json` is forbidden (ADR-0004).

## Authority

- Constitution: `docs/plan/00-engineering-constitution.md`
- Work loop: `AGENTS.md`
- Matrix: `tests/acceptance/matrix.yaml`, `docs/acceptance/matrix.tsv`
