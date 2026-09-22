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
  and `cargo test --locked -p deve-sub-ci`, then
  `cargo run --locked -p deve-sub-ci -- inventory`. The matrix covers every workspace member
  and must run the exact full Cargo command. GitHub job results feed the final
  gate; artifacts retain separate source/run/content provenance. There is no
  shadow planner or Rust execution receipt protocol.

- `python3 scripts/check_architecture.py`: Cargo boundaries, scoped HTTP state,
  optional OpenAPI dependency, source fuse and immutable Action references.
  Reviewed legacy source-size exceptions live only in
  `scripts/architecture-exceptions.json`; they cannot grow silently.
  `python3 scripts/tests/test_architecture.py` verifies forbidden layer edges,
  target/build dependencies and permitted test-only adapters.
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


## Template maintenance verification (2026-09-13)

Scope: M5 native Clash input and lifecycle, M6 pinned cache fallback. Owner:
main implementation lane; three read-only review lanes covered storage/cache,
UI/acceptance and native parsing/security. All accepted findings were verified
and closed. No database migration, production-state mutation or image release
is part of this slice.

- `cargo fmt --all -- --check`, all-target/all-feature `cargo check` and
  `cargo clippy -- -D warnings`: pass. Doc tests: 14 suites, zero examples.
- Full `cargo test --locked --all-targets --all-features`: 90 suites,
  1,025 passed, zero failed, 8 ignored. This includes the revised insert-conflict
  fault injection (version numbers are now allocated transactionally) and
  controlled deletion/rollback serialization. Proxy variables were unset for
  direct loopback tests; TMPDIR used ignored repository scratch storage.
- Native Mihomo routing check: pass with checksum-pinned Mihomo v1.19.0,
  default example and advanced file-rule-provider/DNS/logical-rule/regex fixture.
  This explicitly runs one of the normally ignored tests. Other external
  clients, full protocol validator suites and long soak are not run for this
  slice; these results do not claim end-to-end proxy connectivity.
- Functional matrix: 48 passed, 4 workers, no retries, real isolated servers,
  SQLite and production WASM. Stronger existing-key/order and same-parameter
  failure assertions: 3 affected cases passed. After removing template compiler
  warnings, rebuilt WASM and reran 10 affected desktop/mobile template and
  ordering cases: all passed. Screenshots verified editor/preview sizing,
  scrolling and reachable controls; no browser page errors were reported.
- Existing Playwright suite: 16 passed. Fixture lifecycle: 3 passed.
- Docs/acceptance: 157 cases, 412 verified proof references; architecture gate:
  15 crates passed; CI tooling: 17 unit tests passed. OpenAPI regenerated from
  the current binary and byte-compared with a second export.
- Dependency audit: advisories/bans/licenses/sources pass. The first advisory
  fetch had a TLS interruption; retry without proxy variables succeeded.

Transient command logs are `/tmp/deve-sub-templates-*.log`; functional test
artifacts are under ignored `tests/e2e/test-results/functional-*`. Those local
paths are execution receipts, not durable CI artifact links. Authoritative
repeatable commands and proof entrypoints are retained in the functional
matrix and `tests/acceptance/matrix.yaml`.

## M4 manual category navigation — 2026-09-13

Owner: main agent; one read-only review lane checked category identity, UI state,
pagination and acceptance evidence. Scope: Web node organization, translations,
styles and documentation; no backend schema/API shape or release change.

- Reproduction: a real isolated server with three manual tags (one unassigned)
  exposed only assigned tags through the old dropdown. The new visible-category
  regression failed before the change and passed with the complete tag catalog.
- Functional matrix: 62 passed, 4 workers, zero retries on the final production
  WASM, including 14 new desktop/mobile category checks. Coverage includes empty
  categories, multi-tag counts, rename, removing the last member, deletion, inline
  creation followed by cancellation, pagination, controlled late success/failure,
  error recovery and selection clearing. Adding the 768px viewport assertion
  then rerunning both layout cases: 2 passed.
- Existing Playwright regression: 16 passed, including 10,000-node virtualization,
  language, themes, keyboard navigation, mobile and authentication. Real browser
  screenshots were inspected at desktop, Pixel 5 and tablet widths. Page identity,
  meaningful content, layout, reachable controls and keyboard selection passed;
  no unexpected page exceptions or normal-flow console errors. Injected HTTP 500s
  are expected only in controlled recovery/ordering cases. Browser plugin was
  unavailable; the existing Playwright runner used isolated loopback servers.
- Rust gates: fmt, check, Clippy with `-D warnings`, full all-targets/all-features
  tests and doc tests passed. Full tests: 90 suites, 1,025 passed, 8 ignored.
  Dependency audit: advisories/bans/licenses/sources passed. CSS and release WASM
  builds passed; pre-existing Dioxus component naming warnings remain.
- Docs/acceptance: 157 cases, 420 verified proof references. Architecture gate:
  15 crates passed, including new files and the 500-line fuse. Node translations
  were extracted into their own module; all previous translations remain intact
  except the superseded dropdown label. CI tooling: 17 unit tests passed.
- Read-only review: no unresolved blocker. Initial ordering test route ownership
  was corrected and both delayed-success/delayed-failure paths passed afterwards.
  Final changed/untracked file checks and `git diff --check` passed.

Category counts describe loaded nodes before other filters; the catalog is
complete even if a category's nodes are on a later page. Controlled small node
pages prove this UI boundary without claiming full-database counts. Other browser
engines, external proxy clients, long soak and Docker publishing were not run for
this UI slice. Transient logs use `/tmp/deve-sub-categories-*.log`; browser artifacts
use the runner's ignored per-run directories and `/tmp/deve-sub-categories-*.png`.

## M4 category review fixes — 2026-09-13

Review scope: c42c0b0 and its node-page call paths. Main agent owns fixes;
two fresh read-only review lanes checked the same scope independently. Three
accepted P2 findings were fixed: refresh retaining unloaded/out-of-category
batch targets, late enable/disable completion clearing newer selection, and
node-page failure hiding previously loaded rows and the retry control.

- Before fixes: four real-browser regressions failed on the committed WASM.
  Membership changes used real API requests; pagination and delayed requests
  controlled only transport. One reviewer independently captured the erroneous
  batch target for both membership changes and first-page refresh.
- After fixes: focused category/organization suite 36 passed, including five
  new scenarios on desktop and mobile. Full functional suite: 72 passed,
  4 workers, zero retries. Existing browser suite: 16 passed, including the
  10,000-node virtual list. Same-ID deselect/reselect proves intent revision
  protection, while an unchanged selection still clears on successful completion.
- Rust: 90 suites, 1,025 passed, 8 ignored. fmt, check, strict Clippy and doc tests
  passed; CSS/release WASM build passed. Dependency audit passed. Docs gate:
  157 cases, 427 proof references; architecture gate: 15 crates including all
  new files; CI tooling: 17 tests passed. No schema or API changes.
- Final review: both lanes report no blocker. The independent browser lane
  reran its two original reproductions successfully, with no page exceptions.
  The main agent verified every accepted finding and the final changed/untracked
  diff. Plan wording explicitly limits asynchronous selection cleanup ownership
  to batch enable/disable; tag dialogs continue their existing modal workflow.

Transient evidence is `/tmp/deve-sub-category-review-*.log` and ignored per-run
browser artifacts. This slice does not claim a whole-repository audit, other
browser-engine coverage, long soak, deployment or Docker image publication.

## M8 integration build input correction — 2026-09-13

PR #3 integration CI exposed a source-image build failure: the Web template
editor embeds `examples/templates/clash-routing.yaml`, but the Docker source
stage omitted that directory. The native checkout's WASM and browser tests
passed because the file was available outside the container. The source stage
now copies the template presets before either build branch executes.

The original failure is recorded in GitHub Actions run `34793505608`, Docker
job `103822662218`: `include_str!` could not read the preset under `/build`.
Existing Docker CI owns the regression path: compile the real frontend, boot
the image, verify environment bootstrap, bounded log rotation and health.
This correction changes build inputs only; the M8 release contract and Rust
behavior remain unchanged. The PR's subsequent CI result is the acceptance
receipt; no image publication or production deployment is asserted here.

## M4/M6/M11 independent durability audit fixes — 2026-09-13

Owner: main agent; one independent read-only reviewer audited main
`c3a32c5663b5e3aa36c0347f3ddab6cf6bcae030` and reviewed the resulting fixes.
Scope: CLI restore, source scheduler/cancel API, SQLite generation retention,
their tests and governing documentation. No schema migration, dependency change,
real-data maintenance, release or production deployment is part of this slice.

Three P1 findings were accepted after the main agent independently reran each
baseline binary reproduction:

- BACKUP-003: a previous restore's same-named WAL replaced archived contents
  even though row counts and integrity passed. Each attempt now uses a private,
  unique staging directory on the target filesystem. The final real CLI restores
  `BACKUP_EXPECTED`, preserves the old crash WAL and removes its own staging;
  failed verification preserves the original target. The two new Rust regression
  cases failed before the fix; the complete focused backup suite passed 13 cases.
- OUT-014: nine distinct subscriptions sharing a template caused the first
  selector's sole last-good cache to be evicted; an outage then returned 503.
  Retention now protects matching lenient results for persisted subscriptions,
  including pins and disabled subscriptions, plus active output and eight extra
  candidates. The final real HTTP reproduction retains all nine results and
  returns 200 after the outage. Two storage regressions failed before the fix;
  tests cover repeated generations, shared/changed selectors, disable/delete,
  pins and strict-mode separation. The new API case concurrently generates twelve
  distinct typed selectors and verifies each exact fallback after disabling nodes.
- SRC-009: cancelling an automatic refresh reported success and released its
  lease while its unsignalled runner still published. Scheduled and manual jobs
  now share cancellation registration; missing registration returns retryable 503
  without changing status or lease. The two regression cases failed before the
  fix. The final default scheduler reproduction cancels during blocked Fetching,
  then releases the upstream response: final state is Cancelled with zero new
  snapshots. Tests also cover registration cleanup and a successful replacement.

Final local verification: fmt, check, strict Clippy, all-targets/all-features Rust
tests (92 suites, 1,031 passed, 8 ignored), doc tests and dependency audit passed.
After adding explicit worker-exit synchronization to a test, fmt/Clippy and all
10 source-refresh API tests passed again. The functional matrix passed 73 cases
with 4 workers and zero retries using the final binary and unchanged production
WASM. Docs/acceptance passed 157 cases and 434 proof references; architecture
passed 15 crates including untracked files; CI tooling passed 17 tests. OpenAPI
was generated from the final binary and contains the new cancel 503 response.
The same independent reviewer closed all findings without a new blocker and
checked the cache query plan against the migrated schema.

Transient reproduction and test logs use `/tmp/deve-sub-strict-*.log`; the
original reviewer report and re-review are `/tmp/deve-sub-strict-audit-*.md`.
Browser plugin is unavailable; the existing Playwright runner supplied the
functional regression. Real power-loss durability, large-scale retention latency,
external proxy clients, other browser engines and long-running soak were not
tested locally. This bounded audit does not assert that the entire project is
defect-free. PR CI owns the subsequent Docker/build integration checks.

## M4/M7 background job lifecycle fixes — 2026-09-13

Owner: main agent; the same independent read-only reviewer audited and reviewed
the fixes against main `371da6af318b1b1e7161b955f19fa193b9076250`.
Scope: source scheduler shutdown, probe history persistence, their tests and
governing documentation. No schema migration, dependency or API shape change,
real-data maintenance, release or deployment is part of this slice.

Two findings were accepted after independent baseline reproduction:

- SRC-003/SRC-009: shutdown cancelled the active source refreshes but still
  admitted queued sources with fresh cancellation flags. The scheduler now
  stops admission during a tick and drains its bounded active group. An already
  pending durable start finishes and observes cancellation before fetching.
  Three new regression cases failed on the baseline and passed after the fix;
  the scheduler suite passed all seven cases. A real CLI SIGTERM reproduction
  with nine due sources changed from nine fetches, five new snapshots and five
  completed/four cancelled jobs to four fetches, zero new snapshots and four
  cancelled jobs, with zero unfinished jobs.
- NODE-012: a missing or concurrently deleted node rolled back valid peers'
  latency history, while the run still reported Completed. Missing nodes now
  report skipped; a node deleted after measurement only loses its own history.
  Eligible records commit atomically, and other persistence errors produce
  Failed while retaining measurement diagnostics. Three new real API/SQLite
  tests failed before the fix and passed afterward, covering missing nodes,
  deletion during concurrent measurement and an injected second-insert failure.
  The original real API reproduction now retains the valid node's one history
  record instead of zero and marks the missing node skipped.

Final local verification: fmt, check, strict Clippy, all-targets/all-features Rust
tests (92 suites, 1,037 passed, 8 ignored), doc tests and dependency audit passed.
Docs/acceptance passed 157 cases and 440 proof references; architecture passed
15 crates including untracked files; CI tooling passed 17 tests. OpenAPI was
generated from the final binary. The independent reviewer closed both findings
without a remaining blocker and verified all six before/after regressions.

Transient reproduction and test logs use `/tmp/deve-sub-lifecycle-*.log`;
the final read-only review is
`/tmp/deve-sub-background-job-lifecycle-rereview.md`. No frontend source changed;
browser/WASM/Docker integration is delegated to the subsequent PR CI. Real
power-loss, permanently unwritable storage and long-running soak were not
exercised locally. If storage cannot persist any status, terminal recording
remains best effort and relies on the existing startup recovery path. The
bounded concurrent tests do not exhaust all possible interleavings.

## M5/M6 generation boundaries and concurrent short codes — 2026-09-14

Owner: main agent; one independent read-only reviewer audited and reviewed the
fixes against main `174986ec5ce98b0983eeb30924fb617c88896037`.
Scope: GEN-006, GEN-015 and OUT-013, including their plans, contracts, generated
OpenAPI and regression matrix. No dependency, schema migration, real-data
maintenance, release or deployment is part of this slice.

Three findings were accepted after the main agent reproduced them independently:

- P1 / GEN-006: explicit and quick groups expanded a fixed subscription from
  selected node A to unselected node B. Groups now stay within the effective
  fixed or dynamic selection, report explicit references as `outside_selection`,
  and preserve nested group references. Empty Mihomo groups cannot publish.
- P1 / GEN-015: lenient generation published unsupported Mihomo group types,
  replaced the active good output and failed real client validation. Both modes
  and preview now reject unsupported groups with 422 `incompatible_groups`.
  Active good output and delivery fallback remain unchanged on failure.
- P2 / OUT-013: concurrent regeneration used a stale old-code ID and exhausted
  collision retries against the subscription uniqueness constraint. Replacement
  now deletes the current subscription-owned row as the first write in its
  atomic transaction. The real HTTP test changed from 26 failures in 36 requests
  to 36 successes; exactly one code remains, every prior code returns 404 and the
  current code returns 200. Existing code-collision rollback still passes.

Generation cache keys now include a semantics revision. Old rows remain stored
for normal retention but cannot be served through direct, active or fallback
lookups. Real same-database upgrade testing covers an old pinned cache that
included B: it is regenerated and returns 200 without B. Old invalid active
output is unavailable; if regeneration fails before any current valid cache
exists, delivery returns 503. After rebuilding, a later invalid edit preserves
the current good fallback. Both rebuilt and fallback configurations pass the
installed Mihomo v1.19.0 validator. This deliberately avoids trusting unsafe
legacy output during an upgrade.

All eight new regression tests failed before their respective fixes. Local
baseline checks passed: fmt, check, strict Clippy, all-targets/all-features Rust
tests (92 suites, 1,045 passed, 8 ignored), doc tests and dependency audit. After
the final selected-ID deduplication adjustment, fmt and strict Clippy passed
again, as did the affected generation suite (21 tests), delivery and template
API suites (47 tests), binary build, both real API/upgrade reproductions and
generated OpenAPI comparison. Docs/acceptance passed 157 cases and 448 proof
references; architecture passed 15 crates including untracked files; CI tooling
passed 17 tests. The same reviewer closed all accepted findings with no blocker.

Transient logs use `/tmp/deve-sub-link-review-*.log`; the final read-only review
is `/tmp/deve-sub-link-review-rereview.md`. No frontend source changed; browser,
WASM and Docker integration remain checks for the subsequent PR CI. Real power
loss, long-running soak and every output-profile client were not tested locally.
The bounded concurrent scenarios do not exhaust all possible interleavings.

## M4/M5 independent imports and source cache invalidation — 2026-09-14

Owner: main agent; one independent read-only reviewer audited and reviewed the
fixes against main `f2ef4f6d738a9c639f30a4bfc0e42646cc0960a7`.
Scope: SRC-003, NODE-001/NODE-011 and GEN-015, with their plans, contracts and
regression matrix. No schema migration, dependency, API shape, frontend source,
real-data maintenance, release or deployment change is part of this slice.

- P1 / NODE-011: a remote refresh withdrew an independently imported manual
  node, making fixed-node generation fail. Inserts, active duplicates and
  missing reimports now retain independent provenance in the existing stored
  label. Remote-only labels still derive from bindings; a remote source named
  `manual` does not acquire independent membership. Repeated removal and a
  concurrent import/refresh preserve the ID, credentials, tags and overrides.
- P2 / GEN-015: source rename/deletion changed live filter resolution without
  changing the cache key, so generation returned stale output. Source mutation
  and pool-revision invalidation now commit together; an injected revision
  failure rolls back source metadata and cascading bindings. The source filter
  retains its existing effective-label semantics. Generation semantics v3
  rejects v2 cached output, including active and fallback lookups, while current
  valid output remains available as explicit failure fallback.
- P2 / SRC-003: main CI run `34804296116` exposed a scheduler test that stopped
  the task after a fixed 150ms before it necessarily refreshed anything. A
  controlled 350ms SQLite writer reproduced the failure; waiting for durable
  completion passes under the same contention. Negative eligibility tests now
  include an eligible control; completion and shutdown waits are bounded and
  task exits checked. Production scheduler behavior is unchanged.

Eight new regression cases failed before their respective fixes; the remote
source named `manual` control already passed. Real API reproduction now keeps
manual nodes usable in all three import orders, with identity and user edits
intact. Renaming/deleting a source advances revision and fresh generation
rejects the empty selection instead of returning the old cache. Same-database
upgrade testing rejects stale v2 active output and regenerates under current
rules. Historical missing flags and discarded manual provenance are not
automatically repaired: explicit reimport restores the fixture's original ID.

Final local verification passed: fmt, check, strict Clippy, all-targets and
all-features Rust tests (92 suites, 1,054 passed, 0 failed, 8 ignored), doc tests,
dependency audit and 17 CI-tooling tests. Docs/acceptance passed 157 cases and
458 proof references; architecture passed 15 crates including all three new
test modules. Generated OpenAPI has no diff. The same independent reviewer
closed both production findings without a remaining blocker; its documentation
suggestion about historical missing nodes was applied.

The Browser plugin was unavailable, so the existing Playwright harness ran
against the rebuilt binary and existing Web assets: all 73 functional cases
passed with four workers and no retries, including desktop/mobile flows.
Category screenshots at desktop, tablet and mobile widths were also captured
in the earlier 36-case targeted pass; desktop and mobile images were inspected.
Raw evidence uses `/tmp/deve-sub-node-review-*.log`; final review is
`/tmp/deve-sub-node-review-rereview.md`. Docker/WASM integration and published
branch checks belong to the subsequent PR CI. Real power loss, long-running
soak, every concurrency interleaving and every external client were not tested
locally; this bounded review does not establish that the whole project is free
of defects.

## M4/M5/M6 explicit source withdrawal — 2026-09-15

Owner: main agent; one independent read-only reviewer reviewed the complete
tracked and new-file slice against main
`8b06f0a22873a236a28752264dc94e54c0ef1fe7`. Scope: SRC-001, NODE-011,
GEN-006/015, OUT-008/014 and DEPLOY-001. No real-data maintenance, release or
deployment was performed.

- Source deletion now atomically withdraws exclusive remote nodes, retains
  shared and independently imported nodes, and advances a persistent cache
  withdrawal floor. Injected failure rolls back all changes; concurrent manual
  import preserves identity and user edits. Cache reads, delayed writes and
  activation cannot cross the floor. Current post-deletion output remains
  eligible for later ordinary failure fallback and bounded retention.
- Native Clash and V3 groups retain their names and inbound routes when their
  members disappear. If usable selected nodes remain, an empty group becomes
  select + REJECT. Native dynamic membership is resolved with bounded matching
  and removed from emitted groups so the client cannot exclude the sentinel or
  silently select DIRECT. Overall empty selection returns 503 on public URLs;
  unknown names and invalid filters still fail explicitly. DNS policy order,
  original templates and fixed selection boundaries are preserved.
- The reviewer found surviving nodes' old ID-qualified names could become
  unresolved when deletion removed a naming collision. Parent reproduced and
  fixed ordinary and secondary collision aliases, including the corresponding
  withdrawn-node case. Three before/after reproductions cover this finding;
  exact current names retain priority and aliases cannot expand selection.
- Migration 0028 repairs previously orphaned remote-only records without
  deleting their IDs, encrypted fields, tags or overrides. The recovery test
  builds schema 0027, keeps a backup, upgrades both copies and verifies repeated
  migration is stable. It does not run an old application binary through an
  encrypted-data upgrade. Discarded historical manual provenance still requires
  explicit reimport. The global floor conservatively rebuilds unrelated cached
  selections too.
- The dependency gate detected RUSTSEC-2026-0285, published during this round.
  A separate dependency commit raises rustls to 0.23.45 and its required webpki
  dependency to 0.103.15; the updated audit passes. The fix was checked against
  the [official Rustls advisory](https://github.com/rustls/rustls/security/advisories/GHSA-2mjx-qc3c-rqvc).

Local verification: fmt, check, strict Clippy, all-targets/all-features Rust
tests (93 suites, 1,070 passed, 0 failed, 8 ignored), doc tests and dependency
audit passed. After the last alias regression was added, the affected generation
suite passed all 32 tests and Clippy, check and the binary build passed again.
Delivery/template HTTP suites passed 49 tests. Docs/acceptance passed 157 cases
and 476 proof references; architecture passed 15 crates including untracked
files; CI tooling passed 17 tests. OpenAPI was regenerated from the binary and
adds the admin generation-invalidated 409 response. The independent reviewer
closed the accepted finding with no unresolved blocker.

The fresh binary's real HTTP path passed remote refresh, exclusive/shared/manual
deletion, dynamic and fixed/pinned delivery, token/short/temporary URLs, old and
new ETags, and a restart against the same database. Seven checksum-pinned Mihomo
v1.19.0 cases passed both configuration validation and live group membership
inspection, including nested groups, lookaround, sentinel exclusions and the
original versus materialized membership control.

The rebuilt WASM frontend passed 15 API and 58 desktop/mobile functional cases
with four workers and no retries. Browser plugin was unavailable; local UI
testing used existing Chromium build 1243 via a temporary Playwright config
because the project's build 1234 was absent and its download was slow. PR CI
retains the project-selected browser. Separate real desktop/mobile deletion
smokes verified the revised confirmation, successful deletion and no JavaScript
exceptions; both screenshots were inspected. A smoke locator initially assumed
a dialog role absent from the existing component; the harness was corrected
after inspecting the rendered DOM.

Transient evidence uses `/tmp/deve-sub-withdrawal-*.log`, with final review at
`/tmp/deve-sub-withdrawal-final-review.md`. PR CI owns subsequent Docker and
complete integration checks. No ARM64 runtime, real power loss, long-running
soak or exhaustive concurrency interleavings were tested locally. The checked
regex deadline is not hard preemption of a database await or an individual
match. Clients receive the update on their next pull; already downloaded
configurations remain outside server control.
