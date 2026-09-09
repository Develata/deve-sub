# CI Evidence

## Scope

Machine-readable development evidence for `plan/14-ci-verification.md`.
These scripts are repository tooling, not production CLI APIs.

## Shadow plan

`python3 scripts/ci/plan.py [--base REV] [--output PATH]` emits schema version 1:
commit, source digest/dirty flag, toolchain, lockfile digest, comparison base,
changed paths, selection reasons, reverse consumers, proposed packages/shards,
runtime capabilities and associated case IDs. `execution_profile` is always
`full`; `selection_mode` is always `shadow`. Missing/unreadable base means a
full proposal. An invalid inventory exits nonzero without a successful plan.

Case IDs remain registered even when historical evidence is `not-run` or
`planned`. A case's source reference is an association only. The plan cannot
convert that historical status into a current execution result, execute a
free-text reference, or certify latency/compatibility assertions.

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

## Final result

`python3 scripts/ci/gate.py` consumes GitHub's `needs` job results and the
current event, writes a schema-versioned report, and exits nonzero unless
every mandatory job succeeded. The only event-specific exclusion is multiarch
on PR, reported as `not-run`. Unknown/missing jobs and unexpected skips fail.
The report is job evidence; it never asserts that all matrix cases executed.
Publication still requires the release workflow and its existing approvals.

Reports and logs use synthetic fixtures only. Failure browser traces may carry
disposable test login/session metadata; never connect this harness to production
state. Database files, master keys, persisted storageState and release secrets
must not be uploaded with diagnostic artifacts.
