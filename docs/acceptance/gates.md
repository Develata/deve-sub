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

## Published-image Compose smoke (DEPLOY-001)

On 2026-09-13, the published `ghcr.io/develata/deve-sub:v0.1.0` image passed
on Linux amd64 with Docker Engine 29.7.2 and Compose 5.5.0. Anonymous manifest
inspection and `docker compose pull` used an empty Docker config directory.
The registry index digest was
`sha256:1c0f9d4186307cf323e728a42a2557c630767f00aca5b1bb9f917bbc052bf8c9`;
it contains both amd64 and arm64 manifests. Arm64 execution was not tested here.

The repository Compose input SHA-256 was
`0cabbf7d58b51d22f7c0347ceee32b388f6ea7b7b0ef10b0be0dfdfe458b5365`.
The isolated copy changed only the port mapping to `127.0.0.1:0:8080` and used
a fresh random project name and named volume. Reproduction: pull, run
`up -d --no-build --wait --wait-timeout 60`, execute
`exec -T deve-sub /app/deve-sub health ready`, and request `/` plus the emitted
JS, WASM and CSS under `/assets/`. All returned successfully; Web files were
nonempty. Dioxus links JS from HTML and loads WASM/CSS through the application,
so asset discovery must also inspect the dist asset directory.

After `up -d --no-build --force-recreate --wait --wait-timeout 60`, the new
container was healthy and ready, mounted the same named volume, and retained
the master-key hash and a synthetic persistence marker. The test removed only
its own containers/network/volume afterward. This proves fresh deployment and
same-version recreation; it does not prove a cross-version schema upgrade,
browser interaction or arm64 runtime behavior.

The optional `latest` channel is checked by
`python3 -m unittest discover -s scripts/ci/tests`: the actual promotion shell
step is executed with isolated API/registry doubles for current stable,
prerelease, old release, API failure, missing digest and registry failure.
Compose configuration checks cover the default pinned image, a custom version
and a persisted `.env` selecting `latest`. The existing release index also
passed `imagetools create --dry-run` as a single-source alias, preserving both
architectures. Remote `latest` publication and a pull by that alias are not-run:
the current registry has no such tag, and this environment has no GHCR write
credential. The fixed-version smoke above remains the executed runtime proof.

## Container administrator bootstrap smoke (AUTH-001, DEPLOY-001)

On 2026-09-13, `python3 scripts/tests/test_docker_bootstrap.py --image
deve-sub:bootstrap-smoke-26rb09cv` passed all five scenarios on Linux amd64
with Docker Engine 29.7.2 and Compose 5.5.0. The local validation image used
the published `v0.1.0` runtime/Web above, overlaid with the current
`cargo build --locked -p deve-sub-cli --all-features --bin deve-sub` binary and
`docker-entrypoint.sh`. This exercises the changed CLI and entrypoint in a
real container; it is not a full production-image rebuild or registry release.
CI runs the same script against its freshly built `deve-sub:ci` image.

The script uses the repository Compose file with isolated UUID project names,
ephemeral loopback ports and test-only restart policy `no`. Single-quoted
`.env` credentials containing `$`, and a username beginning with `-`, created
an administrator before HTTP startup. Auth status reported initialized and
login returned 200. Neither bootstrap variable remained in `/proc/1/environ`,
and container logs did not contain the password. Recreating with changed
credentials preserved the original login; the new password/account returned
401. With no configuration, Web setup returned 201 and login returned 200.
Missing username, missing password and a short password each made startup
exit nonzero. Only the script's own containers/network/volumes were removed.

`apps/cli/tests/admin_bootstrap.rs` additionally passed five real subprocess
tests for Argon2id persistence, unchanged disabled users, invalid input leaving
no user, concurrent initialization yielding exactly one user, and non-UTF-8
password errors without secret contents. Manual initialization without
`--if-needed` still rejects a second administrator.

The published `v0.1.0` image does not contain bootstrap support. This evidence
does not establish a new published image, arm64 execution or browser UI
interaction; login/setup were exercised through the real HTTP endpoints.

## Login and subscription credential hardening (AUTH-004/009, SEC-007/009/010, OUT-013)

On 2026-09-13, the final source passed `cargo fmt --all -- --check`,
`cargo check --locked --all-targets --all-features`, Clippy with the same
target/feature selection and `-D warnings`, and the full Rust test command
above: 1004 passed, zero failed, seven ignored. The ignored tests are the
explicit real-process soak and six external client-validator tests; they were
not rerun for this authentication slice. Doc tests passed with zero examples.
The production Web build (`scripts/build-web-release.sh`) and current
all-features CLI build also passed. OpenAPI was regenerated from that binary.

Before implementation, targeted regressions reproduced the missing direct-peer
IP limit, missing no-store policy and malformed-path credential logging.
The final tests cover rotating usernames with forged forwarding headers,
canonical proxy/peer fallback, saturated Argon2 work and cancellation retaining
its permit, isolated short-code probe limits, sensitive response headers and
path redaction. Delivery tests prove new 22-character short codes work and
existing eight-character codes remain valid. Existing ETag and token-grace
tests still pass. Read-only review findings were resolved and verified.

Browser plugin not available; the existing Playwright 1.62.1 workflow used
Chromium 151 with the rebuilt CLI and production WASM, isolated fresh/seeded
databases and desktop/Pixel 5 viewports. Seven selected tests passed using
`--project=real-auth --project=ui-009-mobile --project=ui-authenticated`
and `--grep 'Real browser auth|pending login|UI-009'`. They exercise real login,
wrong-password feedback, reload persistence, repeated Enter suppression and
the mocked 2FA page transition. The desktop/mobile subscription flow checks
missing-code guidance without hiding actions, exact copied short-link delivery,
explicit zero-grace rotation, old-token rejection and continued independent
short-link access. Backend Rust tests cover actual 2FA verification.

A separate 1280x800 Chromium smoke on an ephemeral loopback server verified
page identity, nonblank login/dashboard, no framework overlay, successful login
and HTML no-store. Screenshots were inspected and kept outside tracked files.
There were no JavaScript exceptions or unexpected console warnings/errors;
the sole 401 was the expected initial `/api/v1/auth/me` session probe.
This is local source-build evidence, not a new published image. Docker/arm64,
other browser engines, production HTTPS and long-soak execution were not
repeated for this slice.

## Management functional regression (NODE-001/004/005/006/010/018, SRC-005/013, GEN-003/004, OUT-016)

On 2026-09-13, the new isolated functional matrix passed **28/28**, with four
workers, zero retries, zero skipped tests and zero JavaScript exceptions. The
JSON run identifier was `6a908f3c-9ea9-43c4-ae45-fc3204f2eff4`, duration 60.28
seconds. It used the current all-features debug CLI and production WASM with
Playwright 1.62.1 / Chromium 151, desktop and Pixel 5 viewports. Console reports
contained only the two deliberate HTTP 500 responses in the stale-list tests.
Desktop/mobile tag screenshots were inspected; controls and labels remained
readable, the selected color appeared, and tables stayed inside the page width.

Before fixes, real HTTP/browser checks reproduced duplicate-tag 500 responses,
missing-reference acceptance, invalid names, missing preselection, invalid
subscription defaults and concurrently committed chain cycles. The initial
post-fix API matrix additionally caught SQLite 517 during concurrent import;
acquiring the write transaction before reading dedup identities fixed it.
UI runs exposed missing generated CSS and mobile content expansion. The final
matrix includes page reload, all three batch tag modes, source job interleaving,
request timeout, stale-list failure and cross-template response ordering.

The existing browser suite passed **16/16**, including real authentication,
themes, keyboard access, mobile flows and 10,000-node virtualization. The
lifecycle suite passed **3/3**. Source failure with concurrent admin edits passed
the controlled Notify regression for both current keep_on_fail policies.
The details, commands and transport-double boundaries are in
[functional-matrix.md](functional-matrix.md). Subscription output was fetched;
no real proxy-client import was performed for these management tests.

Final `cargo fmt --all -- --check`, `cargo check --locked --all-targets
--all-features`, Clippy with the same targets/features and `-D warnings`, and
`cargo test --locked --all-targets --all-features` passed: **1005 passed,
zero failed, 7 ignored**. Doc tests passed with zero examples. Docs and
acceptance gates checked 154 cases and 377 proof symbols; architecture checked
15 crates; CI helper tests passed 17/17 and inventory covered all 15 packages.
The full Rust run removed proxy variables only for the test subprocess and
used `TMPDIR=$PWD/tmp/functional-acceptance`: system proxy forwarding of ::1 and
the 3.9 GiB /tmp tmpfs otherwise caused environment failures. The subsequent
complete run passed without skipping those tests. The current CSS regenerated identically with Tailwind
4.3.3 and the pinned npm lockfile; its SHA-256 is
`838bd76d95e857bf1c6e8b2ee1c1c115f124abbd135af579b25f91526f1f928d`.
OpenAPI was exported from the current CLI and includes the new endpoints and
reference-error responses. The final node API regression passed 15/15 after
preserving target-not-found precedence; the six concurrent API cases were
rerun successfully after that correction. `cargo deny --locked check` passed advisory, license, ban and source policies;
`npm audit --prefix apps/web --audit-level=high` reported zero known advisories.
The two read-only review lanes closed all accepted
implementation findings and corrected the OUT-016 proof binding.

These are local source-build results. Existing ignored real-process soak and
external compatibility validators, Docker/arm64 execution, other browser engines,
production HTTPS and registry publication were not repeated for this slice.

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


## M10 log lifecycle verification (2026-09-13)

Owner: primary agent; storage/concurrency and API/UI review lanes closed without
remaining accepted blockers. Scope: AUDIT-001/003/004/005, LOG-001.

- `cargo check --locked --all-targets --all-features` and Clippy with `-D warnings`: pass.
- Full Rust baseline: 89 suites, **1017 passed, 0 failed, 7 ignored**. The existing
  six external client validators and real long-soak test remain not-run; the
  separate 90-second smoke does not promote those to pass. Doc tests: pass.
- `functional.config.ts`: **36 passed**, four workers, zero retries (8 API cases
  plus 14 browser cases at desktop and mobile). Existing browser suite: **16 passed**;
  lifecycle harness: **3 passed**. New cases use isolated old audit fixtures;
  environment retention 90 overrides configuration 0 in the real serve process.
- Storage proves bounded deletion, concurrent confirmation/writing, rollback on
  receipt failure, cancellation while waiting for the write lock, cutoff boundary,
  disabled retention, index selection and pre-migration backup recovery. CLI
  fault injection proves audit failure cannot disable other historical retention.
- Docker rotation against the existing application smoke image (including its
  declared `/app/data` volume): **45000 records written, 26567322 bytes retained**,
  first surviving record 19030, latest marker preserved, **0 anonymous volumes**.
  Declared image volumes are replaced with tmpfs. Only the uniquely labeled test
  container is removed. The same script is registered in the Docker CI job.
- Real 90-second resource smoke: **8039 requests, 0 request failures, 0 error logs,
  3 checkpoint samples**; status PASS. The smoke process explicitly enables the
  maintenance module's debug events so normal application logging can stay quiet.
- Web/WASM build and regenerated pinned Tailwind CSS: pass. Formatting, architecture,
  docs/acceptance gates: pass. Matrix: **157 cases, 150 pass, 7 not-run**, with
  **396 verified proof symbols**. CI helper tests: **17 passed**.

The first browser attempt exposed a test port allocation race with Linux client
ports and an overly exact implicit-label locator; checked ports below the client
range and a role/name locator resolved them. The first resource smoke retained
only info logs and could not see debug checkpoint events; module-level logging
restored the existing assertion. These were rerun successfully, not skipped.

No production audit data or shared system journal was cleaned. Native systemd
journal retention remains host-owned. This verification does not publish a new
image, change an existing container, or run a release workflow.
