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

- CI static topology: `plan/14-ci-verification.md`; evidence format:
  `contracts/ci-evidence.md`. Run `python3 -m unittest discover -s scripts/ci/tests`
  and `python3 scripts/ci/inventory.py`. The matrix covers every workspace member
  and must run the exact full Cargo command. GitHub job results feed the final
  gate; artifacts retain separate source/run/content provenance. There is no
  shadow planner or Rust execution receipt protocol.

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

## Disposable native VM evidence (DEPLOY-002)

On 2026-09-11, `scripts/tests/native_vm.py` passed in a fresh Debian 13.6 amd64
QEMU/TCG guest, systemd 257.13 as PID 1. Cases: fresh installation, mode 0700 data
and 0600 key under the dedicated service account, actual guest reboot, 0.1.0 to
0.1.1 upgrade with exact Web fixture marker, real ExecStartPre rejection and
automatic rollback, failed rollback retaining backups, explicit manual recovery,
SIGKILL after the pending checkpoint, next-run refusal without further mutation,
and recovery from the retained binary/Web/unit. No host account/service changed.

The 0.1.1 binary is an isolated source snapshot with only workspace version
changed; no test release was published and the embedded verification key was
unchanged. Web inputs are the built dist plus distinct fixture markers. Only
release download transport is replaced by a guest fixture; systemctl, accounts,
filesystems, HTTP readiness/version and reboot are real. This evidence is for
the installer, not publisher authentication or signed self-update. UPDATE-001/002
remain not-run at their VM layer; the Rust integration tests still pass.
The separate isolated regression also proves that a successful start followed
by failed old readiness preserves recovery materials.

To reproduce, use a dedicated disposable Debian guest with SSH user `vmtester`,
passwordless sudo, Python 3 and curl; its installed product/data paths must be
empty. A NoCloud seed with an ephemeral authorized SSH key suffices. Use a
checksum-verified official Debian genericcloud image and a separate qcow2 overlay;
QEMU options `-accel tcg -cpu max -m 1024 -smp 2` work without KVM. Forward a
loopback-only host port to guest port 22, and bound the VM lifetime with `timeout`.
The genericcloud kernel has no 9p support; the test transfers a fixture via SCP.
Do not point this harness at an existing installation.

```sh
python3 scripts/tests/native_vm.py --port 24681 --key /path/to/ephemeral-key \
  --known-hosts /path/to/verified-guest-hosts \
  --binary-a /path/to/real-0.1.0 --binary-b /path/to/isolated-real-0.1.1 \
  --web apps/web/dist --output /tmp/native-vm.json
```

The final tested installer SHA-256 was
`5b52650845dd4b334faf3236035972aca2bf920b80652b2ef31435d9f45169d0`.
The harness records input binary and installer hashes with exact case outcomes.
Keep diagnostic JSON; do not publish VM disks, keys or fixture databases.
Power loss during multi-file replacement is not atomic. The pending checkpoint
requires operator recovery; this test does not establish automatic boot recovery.

## Long soak and control-plane boundaries

The separate `optional-long-soak` workflow runs 1800 seconds weekly or manually,
with a 75-minute job limit and 90-day JSON retention. The ordinary CI gate retains
90 seconds. Reports include initial/peak/final RSS, FD, WAL and DB, per-cycle and
per-second tail slopes, page/freelist counts, history rows and task/limiter counts.
A failed assertion retains partial numeric evidence and never reports PASS.
Freelist is reusable SQLite capacity, not a leak; there is no automatic VACUUM.

Local measurement on 2026-09-11: 1800.062 seconds, 51,641 cycles, 242,732
requests, zero unexpected failures or error logs, graceful shutdown PASS.
RSS initial/peak/final was 40,996,864 / 65,466,368 / 60,534,784 bytes;
FD 18 / 25 / 21; WAL 1,133,032 / 4,371,352 / 4,371,352 bytes;
DB 512,000 / 75,923,456 / 75,923,456 bytes. Tracked jobs peaked at 1 and
finished at 0; panics/cancellations stayed 0; limiter entries peaked/finished
at 5,165. Final page/freelist counts were 18,539 / 0. RSS tail slope remained
positive at 621.626 bytes/second; FD tail slope was zero. This is one measured
envelope, not proof of zero slow leak or a cross-run trend. The run accumulated
51,641 traffic observations and 5,165 refresh/probe rows each inside retention;
DB growth here does not measure expiry reclamation throughput. Final explicit
closure of the harness's SQLite sampling connections was separately exercised
by a 10-second real-process smoke after this long run had started.

The 2026-09-11 read-only GitHub audit found no main protection, repository
rulesets or release-environment protection rules. Repository code does not fix
these settings. Operator action remains: prohibit main force-push/deletion,
require `acceptance-gate`, protect immutable `v*` tags and configure the intended
release environment. No additional reviewer quota or commit-signing ritual is
required by this round; publisher authentication remains the signed manifest.
