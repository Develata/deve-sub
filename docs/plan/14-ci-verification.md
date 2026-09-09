# 14 — CI Verification

## Scope and authority

This chapter owns CI scheduling, impact analysis and artifact handoff for M8.
It preserves the constitution's verification and release constraints. The
machine evidence boundary lives in `contracts/ci-evidence.md`; ADR-0008 records
the original parallelization decision. Product behavior and the production
`deve-sub` command surface do not change.

## Execution and impact graphs

The initial impact planner is **shadow-only**: every supported CI event still
runs the full existing verification suite. Its proposed package/shard/case
selection is diagnostic information, never an input to job conditions. The
workflow's static Rust matrix owns package partitioning; metadata must prove
that each workspace member belongs to exactly one shard. All-targets,
all-features and the separate doctest gate remain intact.

Impact analysis includes normal, dev and build dependencies, conservatively
unioned across targets/features. Reverse reachability includes test consumers;
cycles in that graph do not become job dependency cycles. Runtime consumers
(validators, binary/WASM browser tests, soak and Docker) are explicit edges.
Case references associate proof with owners; they are not executable commands
and do not prove that an assertion ran.

Unknown paths or an unavailable comparison base propose full execution. Public
contracts/authority, domain/kernel/security boundaries, manifests/lockfile,
toolchain/build configuration, migrations/recovery, shared fixtures and
CI/acceptance rules require full execution. Main, nightly, release, deployment
and recovery always require full execution. A malformed graph, missing member,
duplicate shard owner or invalid case association fails the planner; a full
fallback cannot turn missing verification into a pass.

## Scheduling and final gate

Static checks, Rust tests, candidate binary construction and WASM construction
may run concurrently. A candidate build is unprivileged verification input,
not an approved release. Runtime consumers wait for their exact artifacts;
Docker continues to build from source. The release workflow still waits for
the complete reusable CI workflow before constructing distribution binaries,
signing or publishing. No publication permission moves into candidate jobs.

The final `acceptance-gate` always evaluates every required CI job, including
the planner, documentation, validators, supply-chain, build and runtime jobs.
Only the existing PR exclusion of multiarch is permitted; it is recorded as
not-run, never pass. A missing job, unexpected skip, cancellation, timeout or
failure blocks the gate. Job success does not promote historical matrix
statuses or claim all registered acceptance cases passed.

## Artifacts and caches

Binary and WASM consumers verify the producer manifest before execution. The
manifest binds commit, source identity, toolchain, target/profile/features,
lockfile, run identity and exact file hashes/sizes. Unexpected, absent,
symlinked or changed files fail verification. Artifact transfer checks are
explicit errors rather than relying on a download action warning.
The manifest's own digest is a producer job output, checked separately from
the downloaded payload before trusting its file inventory.

Docker's backend and frontend stages continue to compile from source. Stable
Dioxus installation precedes source copies; a platform-independent WASM stage
is shared by both runtime architectures. Docker and multiarch have separate
cache write scopes and may read each other's cache. Cache misses change cost,
not verification scope or truth. Published artifacts are never identified by
cache keys alone.

## Runtime isolation and failure/recovery

Browser invocations own their ports, temporary databases/keys, identities,
fixtures, child processes and logs. Setup and teardown use the same run-local
configuration; partial setup failure stops the children already started.
Workers within one invocation remain serial while they share seeded state.
Failure reports preserve logs and browser diagnostics without uploading DBs,
master keys or persisted storageState files. Browser traces can contain this
invocation's disposable synthetic login/session metadata; the fixture database
is destroyed at teardown and no production state is used. No cleanup targets
another invocation's files.

On planner or evidence failure, rerun the complete gate after correcting the
cause. On a cache failure, rebuild from source. On artifact mismatch, reject
the artifact and rebuild; do not silently regenerate provenance for it. No
failed optimization permits publishing a candidate or skipping a baseline.
Artifact identity includes the workflow attempt: retry the whole workflow,
not only a failed consumer with a previous attempt's candidate artifact.

## Verification and migration

`python3 -m unittest discover -s scripts/ci/tests` exercises missing/duplicate
members, reverse test consumers, unknown changes, invalid evidence, unexpected
skips and artifact corruption. `python3 scripts/ci/plan.py` exercises real
metadata/workflow/case inventory. Browser smoke exercises isolated lifecycle.
Existing Rust, docs, compatibility, browser, soak and Docker gates remain.

Enable selective PR execution only after shadow/full comparisons demonstrate
complete mappings and per-shard execution receipts exist. The current phase
does not satisfy that activation condition. Main/nightly/release full baselines
remain mandatory even after future PR activation. Fine-grained case execution,
an additional product CLI and publication from promoted image digests require
their own proved implementation; they are not implied by shadow selection.
