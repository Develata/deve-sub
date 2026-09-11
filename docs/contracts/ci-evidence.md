# CI Evidence

## Scope

Machine-readable development evidence for `plan/14-ci-verification.md`.
These scripts are repository tooling, not production CLI APIs.

## Static inventory

`python3 scripts/ci/inventory.py` validates the complete static workflow against
Cargo workspace metadata. Every package occurs once; the exact Cargo command
and flags are required, with no conditional/error-suppressed Rust test step.
There is no shadow plan schema or selective execution surface.

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

`python3 scripts/ci/gate.py --output PATH` consumes GitHub `needs` and the event.
It writes schema version 3 with complete job outcomes, exiting nonzero unless
every required job succeeded. Only multiarch on PR may be skipped, reported as
`not-run`. Unknown/missing jobs and unexpected skips fail. The report never
asserts that all acceptance cases ran. Older receipt reports remain historical.
Publication still requires the release workflow and operator authorization.

Reports and logs use synthetic fixtures only. Failure browser traces may carry
disposable test login/session metadata; never connect this harness to production
state. Database files, master keys, persisted storageState and release secrets
must not be uploaded with diagnostic artifacts.
