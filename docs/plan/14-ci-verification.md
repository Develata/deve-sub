# 14 — CI Verification

## Scope and authority

This chapter owns CI scheduling, impact analysis and artifact handoff for M8.
It preserves the constitution's verification and release constraints. The
machine evidence boundary lives in `contracts/ci-evidence.md`; ADR-0008 records
the original parallelization decision. Product behavior and the production
`deve-sub` command surface do not change.

## Static execution and final gate

Every supported event runs the full baseline. The workflow's static matrix
owns package partitioning; Cargo metadata proves that every workspace member
belongs to exactly one shard. Inventory validation also requires the exact
locked/all-targets/all-features Cargo command, without conditions, filters,
error suppression or additional shell commands. Doctests remain a separate gate.
Selective CI and shadow impact planning are not enabled.

Repository-only `deve-sub-ci` owns static inventory and final result evaluation.
It is a small Rust workspace binary with no application dependencies, not a
production subcommand or a second scheduler. GitHub Actions still owns process
execution. Artifact transfer and OS/browser-specific tooling retain their
existing Python/TypeScript entrypoints.

Browser execution has four static lanes: legacy UI (including process lifecycle
checks), functional API, functional desktop and functional mobile. All lanes
must run, with `fail-fast: false` and at most four runners. The shared-state
legacy suite remains serial; isolated functional lanes use two workers each on
CI to bound simultaneous servers and password-hashing memory on each runner.
Inventory rejects missing/duplicate lanes and weakened commands. Each lane
uploads a uniquely named diagnostic artifact; the job aggregate blocks the
final gate if any lane fails or is cancelled.

Only superseded runs for the same PR share a workflow concurrency group and
cancel one another. Non-PR groups include the unique workflow run ID: using a
branch-only group would replace pending main/release baselines even when
`cancel-in-progress` is false. Every job has an explicit 5–60 minute timeout;
inventory rejects missing or unbounded deadlines. Runtime consumers continue
to use their finer request/test/startup/teardown deadlines.

Static checks and candidate builds may run concurrently. Runtime consumers
wait for their exact artifacts; Docker still builds from source. Distribution
builds, signing and publication wait for the entire reusable CI workflow.
The final `acceptance-gate` always evaluates every required GitHub job.
Only multiarch on PR may be skipped, reported as not-run. Missing jobs,
unexpected skips, cancellation and failure block acceptance.

The convergence round removes the shadow planner and Rust execution receipts.
The planner did not reduce execution or provide measured selection benefits.
Receipts caught command/source drift, but were unsigned statements produced by
the same trusted runner. Static command/partition checks now catch omitted
packages, wrong flags and replaced commands; GitHub owns execution status and
cancellation. This deliberately trusts the reviewed workflow and runner. It
does not independently attest to a compromised runner or a test that mutates
its checkout. Such a test could also forge an unsigned receipt.

Artifact provenance remains necessary: a successful producer job alone cannot
prove that a consumer downloaded the exact payload from the same source and
workflow attempt. Final aggregation does not promote historical acceptance
statuses or claim every registered case ran. Future selective execution would
need new evidence of coverage and meaningful savings; no dormant planner is
maintained in anticipation of that work.

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
Legacy UI workers within one invocation remain serial while they share seeded
state. The functional matrix uses a separate fully-parallel Playwright config:
each test owns a server, database, key, session and fixtures. Four workers are
the default bound. Same-database race scenarios issue simultaneous requests
inside one test and assert committed invariants, not response status alone.
Every request, startup, test and teardown has a finite deadline. Failure
artifacts include the stable case ID and isolated server log. Retries are off;
failures cannot be hidden by automatic reruns. The case-to-feature mapping
lives in docs/acceptance/functional-matrix.md and binds existing acceptance IDs.
Failure reports preserve logs and browser diagnostics without uploading DBs,
master keys or persisted storageState files. Browser traces can contain this
invocation's disposable synthetic login/session metadata; the fixture database
is destroyed at teardown and no production state is used. No cleanup targets
another invocation's files.

On inventory or evidence failure, rerun the complete gate after correcting the
cause. On a cache failure, rebuild from source. On artifact mismatch, reject
the artifact and rebuild; do not silently regenerate provenance for it. No
failed optimization permits publishing a candidate or skipping a baseline.
Artifact identity includes the workflow attempt: retry the whole workflow,
not only a failed consumer with a previous attempt's candidate artifact.

## Verification

`cargo test --locked -p deve-sub-ci` checks missing/duplicate members and browser
lanes, weakened commands, deadlines, cancellation policy and unexpected skips.
`cargo run --locked -p deve-sub-ci -- inventory` verifies the real metadata and
workflow partition; its Cargo metadata child has a 60-second deadline and is
killed/reaped on timeout. `python3 -m unittest discover -s scripts/ci/tests`
retains artifact corruption and release-policy tests.
Existing Rust, docs, compatibility, browser, soak and Docker gates remain.
The docs gate checks that historical pass cases have existing proof references.

Dioxus CLI uses an isolated install-root cache keyed by runner OS/architecture,
runner image generation, project Rust version and CLI version. Other installed
Rust toolchains and Cargo.lock do not invalidate this tool-only cache. A hit
must report the expected version; a miss builds from locked source. Cache hits
never substitute for WASM build, artifact verification or runtime acceptance.

Long application soak is a separate optional manual/weekly workflow. The main
baseline keeps its 90-second regression soak. Long-run reports retain time and
work-normalized slopes and SQLite free/page counts; no full VACUUM is automatic.

The Web job also regenerates the committed Tailwind CSS using the pinned
`apps/web/package-lock.json` and requires an empty generated diff. CSS remains
a committed build input so native/Docker release builds do not require Node.
After Rust class names or input.css change, run `npm ci --prefix apps/web` and
`npm run --prefix apps/web build:css` before building WASM.
