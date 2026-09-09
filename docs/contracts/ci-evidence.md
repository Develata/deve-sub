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

## Rust shard execution receipts

`python3 scripts/ci/run_shard.py --shard NAME --packages="-p PACKAGE ..."
--output PATH` runs exactly `cargo test --locked --all-targets --all-features`
with the listed packages. It uses argv execution, never a shell. A receipt is
written before spawning and finalized when Cargo terminates. Missing compiler,
spawn errors, nonzero exit, cancellation and changed source identity cannot
produce a passing receipt. Cancellation stops the invocation's process group.

Schema version 1 binds `kind: rust-shard-execution`, source identity (including
all repository fixtures), workflow run/attempt, shard, ordered packages and
argv, `profile: test`, Cargo-default target selection, actual rustc version/host,
UTC start/finish timestamps, monotonic elapsed milliseconds, exit code, status
and a bounded diagnostic. The initial `running` state and terminal `fail` or
`cancelled` states are non-pass. Only a completed command with exit code zero
and unchanged source may report `pass`. No ambient environment or credentials
are collected. This is command evidence; it does not enumerate passing cases
or turn Cargo's ignored tests into passes.

Each job uploads `rust-execution-NAME/receipt.json`. The final gate downloads
these artifacts without merging their directories and requires exactly the
static matrix's shard set. Unknown fields, malformed values, files/symlinks,
source/run/command/compiler mismatches, missing receipts and non-pass results
are errors. The current runner/verifier supports Linux amd64 Rust shards;
cross-platform container validation remains in the separate multiarch job.
GitHub job results are checked independently; a receipt cannot
override a failed job. Receipts are diagnostic evidence from the workflow,
not a publisher signature or a cryptographic attestation of execution.

## Final result

`python3 scripts/ci/gate.py --receipts ROOT --output PATH` consumes GitHub's
`needs` job results, the current event and the complete Rust receipt set.
It writes schema version 2 with job and shard outcomes, and exits nonzero unless
every mandatory job and Rust receipt succeeded. Historical version 1 reports
remain historical evidence; they cannot substitute for current receipts.
The only event-specific exclusion is multiarch
on PR, reported as `not-run`. Unknown/missing jobs and unexpected skips fail.
The report is job evidence; it never asserts that all matrix cases executed.
Publication still requires the release workflow and its existing approvals.

Reports and logs use synthetic fixtures only. Failure browser traces may carry
disposable test login/session metadata; never connect this harness to production
state. Database files, master keys, persisted storageState and release secrets
must not be uploaded with diagnostic artifacts.
