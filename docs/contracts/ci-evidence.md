# CI Evidence

## Scope

Machine-readable development evidence for `plan/14-ci-verification.md`.
These tools are repository tooling, not production CLI APIs.

## Static inventory

`cargo run --locked -p deve-sub-ci -- inventory` validates the fixed
ci-non-browser workflow against Cargo workspace metadata. Every package and
required non-browser job occurs once, with exact commands, bounded deadlines,
no error suppression and the existing cancellation/multiarch policy. Pure HTTP
functional API15 and Node lifecycle3 retain their configs; Linux installer stays
required. Actual browser suites are owned by the local-browser runner, not
omitted registered cases. Profile identity is part of every aggregate result.
The separate CI check is ci-non-browser-gate; it never reports full acceptance.
An old required acceptance-gate is not renamed or weakened by this change.

Docker runtime verification is also part of the static inventory: the regular
Docker job and exactly two native multiarch lanes must build the original
Dockerfile into the existing default Docker daemon and run the matching image
smoke. The lanes bind amd64 to `ubuntu-24.04` and arm64 to `ubuntu-24.04-arm`;
both finish even if one fails. Extra setup actions, privileged builders, binfmt
registration, altered commands or missing lanes fail inventory. Building ARM64
without starting it is not DEPLOY-004 evidence.

The common browser preparation module checks all installed Playwright package
versions against the lockfile and selects only the full Chromium registry
entry. It requires an existing regular executable with the exact browser
version, records its SHA-256, launches that explicit path with a bounded
Playwright launch, verifies the connected browser version, closes it and checks
the digest again. Only then may the CLI export its path to every browser config
and report READY with `launch_verified: true`. Headless full Chromium is not
the separate headless-shell binary. Neither system browsers nor alternate
browser selectors are accepted. The local caller bounds preparation to one minute.

The asset provider is the existing Playwright cache, not a downloader. This
workflow configures no authorized provisioning source; asset origin/integrity
must be established separately, and a measured digest is not publisher
authentication. A missing asset, lock/version mismatch, failed launch/close or
changed executable produces NOT_RUN and a nonzero exit, with no path export.
Local browser suites prepare before launching browsers. HTTP-only and Node
lifecycle tests use the Playwright runner without preparation or a browser;
the independent Linux installer remains in CI.
The unchanged final gates reject missing execution evidence; preparation unit
fixtures are not real-browser acceptance.

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

`cargo run --locked -p deve-sub-ci -- gate --profile ci-non-browser --output
PATH` consumes GitHub needs/event, requires every fixed non-browser CI job and
writes the explicitly scoped profile. Default full evaluation still requires
the old complete inventory; it rejects this reduced job set. No removed browser
job is represented as successful or allowed-skip. All reports retain
full_acceptance_readiness: NOT_ASSERTED. Release's separate acceptance-gate
combines actual non-browser CI success with verified same-clean-commit local
browser evidence, failing before build when that input is missing.

Reports and logs use synthetic fixtures only. Failure browser traces may carry
disposable test login/session metadata; never connect this harness to production
state. Database files, master keys, persisted storageState and release secrets
must not be uploaded with diagnostic artifacts.

## Normal regression results

`python3 scripts/ci/normal.py inventory|run|gate` owns the OS/browser result
adapter for `plan/14-ci-verification.md`'s normal capability lanes. The static
catalogue is `tests/acceptance/normal-ci.json`: 39 browser/API tests and one
controlled DNS handshake, assigned once across 11 required lanes. The Rust inventory preserves the complete catalogue and checks the explicit
DNS-only CI command, producer dependencies and always-running scoped gate.
Protocol validator prerequisites remain with the local browser suite. The default full normal evaluator still requires all11 lanes/40 cases. CI
uses explicit ci-non-browser and separate normal-non-browser/gate jobs: DNS1
only, with browser scope NOT_RUN. Cross-profile reports are rejected.

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

## Local browser bundle

The local-browser catalogue references the unchanged 39 normal cases and lists
exact legacy/functional viewport case identities. All151 primary browser cases
must appear exactly once in raw Playwright results and pass without retries,
skips or global errors. Source identity before/after execution, source-bound
producer manifests and their independent expected hashes, exact executable
path/version/SHA and real Playwright launch/close evidence are required. The
prepared path is passed explicitly to each child. Runtime Compose remains a
separate registered external-prerequisite case.

The verifier reads an already-held regular directory, never a remote URL. The
operator-approved manifest SHA travels separately from the bundle. Every
included regular file must match its declared size/hash; links, traversal,
unknown/missing files and malformed results fail. Reports are runner-authored
execution evidence, not independent attestation. Release requires the same
clean commit/source/lock/toolchain as its checkout; a development dirty run is
not a release certificate. The existing Linux binary/Web producer contract is
retained; local macOS support is not inferred from browser availability.

Release reads `$RUNNER_TEMP/local-browser-evidence` and the separately reviewed
`LOCAL_BROWSER_MANIFEST_SHA256` repository variable. No supplier/transfer step is
configured in this candidate: without an approved populated input both tag and
manual release fail. Choosing/supplying that immutable handoff and changing any
required-check setting are separate explicit owner actions, not automatic
consequences of publishing this private patch.
