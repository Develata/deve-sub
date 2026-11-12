# CI Evidence

## Scope

Machine-readable development evidence for `plan/14-ci-verification.md`.
These tools are repository tooling, not production CLI APIs.

## Static inventory

`cargo run --locked -p deve-sub-ci -- inventory` validates the complete static
workflow against Cargo workspace metadata. Every package occurs once; the exact
Cargo command and flags are required, with no conditional/error-suppressed Rust
test step. The four browser lanes cover legacy, functional API, desktop and
mobile exactly once; lane commands and the legacy lifecycle command are checked.
Rust and browser matrices disable fail-fast. Browser execution is bounded to
four lanes and two workers per functional lane; legacy keeps one worker.
All required jobs have finite explicit timeouts, and only obsolete same-PR
workflow runs may cancel one another. There is no shadow plan schema or selective
execution surface. This workspace-only binary is not packaged in product releases.

Docker runtime verification is also part of the static inventory: the regular
Docker job and exactly two native multiarch lanes must build the original
Dockerfile into the existing default Docker daemon and run the matching image
smoke. The lanes bind amd64 to `ubuntu-24.04` and arm64 to `ubuntu-24.04-arm`;
both finish even if one fails. Extra setup actions, privileged builders, binfmt
registration, altered commands or missing lanes fail inventory. Building ARM64
without starting it is not DEPLOY-004 evidence.

Browser preflight accepts only an existing regular executable for the locked
Playwright revision and exact browser version, records its SHA-256 and exports
its path for every browser config. It does not download or choose a substitute.
Absent/mismatched assets fail preflight and the final gate; no browser cases
are claimed to have run. The installer and process lifecycle checks precede
this capability check on the legacy lane.

## Artifact manifest

`python3 scripts/ci/artifact.py create|verify --kind binary|wasm --root PATH`
uses `.ci-artifact.json` within a dedicated artifact directory. Schema version
1 binds commit, source digest, toolchain, Cargo.lock SHA-256, target, release
profile, features and workflow run/attempt to each file's relative name,
SHA-256 and byte count. The binary directory contains only `deve-sub`; WASM
contains the complete frontend dist. Consumers require the same source and
run identity and verify the exact file set. Hashes prove identity/integrity,
not a publisher signature or completed runtime acceptance.
Verification requires `--manifest-sha256` from the producer's separate job
output. A manifest shipped inside a replaced artifact cannot certify its own
replacement contents; empty, stale or mismatched expected digests fail closed.

## Job results

Rust execution receipts are removed. GitHub owns job completion; the static
inventory guards command and package drift. These checks trust the reviewed
workflow and its runner, rather than treating a runner-authored receipt as
independent execution authentication.

`cargo run --locked -p deve-sub-ci -- gate --output PATH` consumes GitHub `needs`
from `CI_NEEDS` and the event from `GITHUB_EVENT_NAME`.
It writes schema version 3 with complete job outcomes, exiting nonzero unless
every required job succeeded. Only multiarch on PR may be skipped, reported as
`not-run`. Unknown/missing jobs and unexpected skips fail. The report never
asserts that all acceptance cases ran. Older receipt reports remain historical.
Publication still requires the release workflow and operator authorization.
The JSON schema is unchanged by the Rust migration. A failed or cancelled
browser matrix lane makes its required aggregate unsuccessful; the gate cannot
convert that result into a pass. The gate runs with `always()` and writes its
report before returning failure; missing setup/toolchain inputs still fail the
job rather than manufacturing a successful report.

Reports and logs use synthetic fixtures only. Failure browser traces may carry
disposable test login/session metadata; never connect this harness to production
state. Database files, master keys, persisted storageState and release secrets
must not be uploaded with diagnostic artifacts.

## Normal regression results

`python3 scripts/ci/normal.py inventory|run|gate` owns the OS/browser result
adapter for `plan/14-ci-verification.md`'s normal capability lanes. The static
catalogue is `tests/acceptance/normal-ci.json`: 39 browser/API tests and one
controlled DNS handshake, assigned once across 11 required lanes. The Rust
inventory checks the complete lane set, execution command, build dependencies,
validator installation and always-running gate. The final baseline also
requires `normal-acceptance` and `normal-gate`.

Each lane's `summary.json` records source identity, workflow run/attempt,
command, binary digest, exit code, exact case IDs/statuses, timestamps and raw
report digest. The aggregate verifies complete unique case coverage and the
raw report, and requires GitHub success for both producers, the Rust gate and
the normal matrix. Playwright expected failures, skips and retries never count
as pass. Errors write a failing aggregate before nonzero exit. Missing evidence
is failure, including if setup prevented a lane from writing its summary.
These reports trust the reviewed test runner; they are not signed attestations.

The scope is implemented automated regression, not every row in the broader
manual acceptance catalogue. `external_scope` and
`full_acceptance_readiness: NOT_ASSERTED` survive aggregation. There is no
promotion of historical results, native device claims from browser viewports,
or inference that unsupported/unimplemented combinations have passed.

Sources reviewed for this implementation:
[GitHub job matrices](https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/running-variations-of-jobs-in-a-workflow),
[artifact handoff](https://docs.github.com/en/actions/tutorials/store-and-share-data),
[status expressions](https://docs.github.com/en/actions/reference/workflows-and-actions/expressions),
[SHA pinning](https://docs.github.com/en/actions/reference/security/secure-use),
and [Playwright test-level sharding](https://playwright.dev/docs/test-sharding).
