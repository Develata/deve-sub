# ADR-0005: Security Field Three-State Semantics

- **Status**: Accepted
- **Date**: 2026-08-02

## Context

Proxy protocols encode TLS certificate verification as `allowInsecure` (URI),
`skip-cert-verify` (YAML/JSON), or similar fields. This parameter has three
distinct meanings:

1. **Not provided** — the client uses its default behavior (typically secure).
2. **Explicitly false** (`allowInsecure=0`) — the user requires certificate
   verification.
3. **Explicitly true** (`allowInsecure=1`) — the user explicitly disables
   certificate verification.

Compressing these into a single `bool` with a default loses the distinction
between "not provided" and "explicitly false." This matters because:

- Some target clients behave differently when the field is omitted vs. set to
  `false`.
- Auto-filling a default can silently change security semantics (constraint #8).
- Compatibility conversion must not upgrade an absent field to `true` or
  downgrade an explicit `false` to absent.

## Decision

Model `skip_cert_verify` as `Option<bool>`:

- `None` — the parameter was not provided. The system must not silently fill in
  a default.
- `Some(false)` — explicitly secure. Certificate verification is required.
- `Some(true)` — explicitly insecure. Certificate verification is skipped.

**The system must never auto-set `Some(true)` for compatibility.** If a target
client does not support certificate verification control, the node is either
excluded with a compatibility report or the field is left absent — never
silently set to insecure.

## Consequences

- Parsers must distinguish field absence from `false`. This requires careful
  YAML/JSON/URI parsing (e.g., checking key presence, not just truthiness).
- Emitters must output the correct representation per target format: omit the
  field for `None`, output `allowInsecure=0` for `Some(false)`, output
  `allowInsecure=1` for `Some(true)`.
- The three-state model is the authority; any code that collapses it to a
  boolean is implementation drift.
- Round-trip tests must verify that all three states survive a
  parse → model → emit cycle (acceptance: `PARSE-014`, `PARSE-015`).

## Alternatives considered

1. **`bool` with default `false`** — rejected: loses the "not provided" state;
  cannot distinguish absence from explicit false; may break clients that behave
  differently when the field is omitted.
2. **`bool` with default `true`** — rejected: insecure default violates the
  safety priority; would silently disable certificate verification for nodes
  that did not request it.
3. **`enum { Default, Require, Skip }`** — rejected: isomorphic to
  `Option<bool>` with no additional expressiveness; adds unnecessary type
  complexity.
