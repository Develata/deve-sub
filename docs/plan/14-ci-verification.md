# 14 — CI Verification

## Scope and authority

This chapter owns CI scheduling, impact analysis and artifact handoff for M8.
It preserves the constitution's verification and release constraints. The
machine evidence boundary lives in `contracts/ci-evidence.md`; ADR-0008 records
the original parallelization decision. Product behavior and the production
`deve-sub` command surface do not change.

## Static execution and final gate

Every supported CI event runs the explicit `ci-non-browser` profile. The
verification backup branch is a supported push target, not publication authority.
Rust shards retain the exact locked/all-targets/all-features command and every
workspace member once. Rust, audit, WASM, compatibility, DNS, native installer,
Docker, soak, HTTP API and process-lifecycle checks remain required. No browser
installation, preflight or browser invocation runs on GitHub runners.

Execution follows actual fixture dependencies. Functional API has 15 HTTP-only
cases; lifecycle has three Node/filesystem/socket failure-path cases; DNS has
one controlled handshake. Those 19 cases stay in CI. Normal's nominal API and
protocol cases request page/browser fixtures and remain local with the other
browser cases. Playwright's test runner and discovery do not by themselves
require a browser. The registered primary inventory remains 170 cases: 19 CI
and 151 local-browser cases (16 legacy, 96 functional viewport cases, 39 normal).
The separate Compose-browser runtime case retains its existing external/runtime
prerequisites and is reported separately, not silently removed or promoted.

The CI result is named `ci-non-browser-gate`, never `acceptance-gate` or profile
`full`. Missing/failed/cancelled required CI jobs still fail; only the existing
PR multiarch exception is reported not-run. The old full-profile evaluator
remains strict, so non-browser-only results cannot pass it. Required-check
configuration is not edited: an owner must explicitly review any check-name
migration; an old required acceptance-gate remains unsatisfied until then.

The normal catalogue remains all 11 lanes/40 cases. Its default full profile
still requires complete discovery and execution evidence. The explicit
ci-non-browser profile executes only DNS and reports the other scope as NOT_RUN;
it cannot be aggregated as full. HTTP/API and lifecycle keep their original
configs, IDs, fixtures and assertions, with no browser provisioning or selector.
The isolated native installer remains a required Linux bubblewrap job.

Repository-only `deve-sub-ci` owns the fixed CI job inventory and profile result.
Python owns normal/browser result adaptation; test configs retain their
existing fixtures. There is no generic scheduler or selective shadow planner.

Only superseded runs for the same PR share a workflow concurrency group and
cancel one another. Non-PR groups include the unique workflow run ID: using a
branch-only group would replace pending main/release baselines even when
`cancel-in-progress` is false. Baseline and release execution jobs have explicit
5–60 minute timeouts; inventory and release-policy tests reject missing or
unbounded deadlines. Runtime consumers continue
to use their finer request/test/startup/teardown deadlines.

Static checks and candidate builds may run concurrently. Runtime consumers
wait for their exact artifacts; Docker still builds from source. Distribution builds, signing and publication require both the non-browser CI
and a separate `acceptance-gate` in the release workflow. That full gate checks
an independently approved digest of a local-browser evidence bundle from the
same clean commit/source/lock/toolchain and source-bound binary/Web producers.
It validates actual full151 startup and all 151 exact raw case results, with
no skips/retries. Missing, stale or corrupt evidence fails closed before build.
A passing CI job is not full acceptance. The external evidence handoff is not
configured or authorized by this source change; release stays blocked until
an owner approves and supplies the immutable bundle and independent digest.

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
is shared by both runtime architectures. The ordinary Docker job and both
multiarch images use the same bounded runtime smoke: verify the loaded
architecture and default internal health command,
observe Docker become healthy within 60 seconds, and exercise live/ready/Web
HTTP plus the non-root user. Run both architectures even if one smoke fails;
the combined result fails if either does. Each invocation owns a random
container, loopback port and tmpfs data, and cleans up on success, failure or
handled termination. These checks implement M8 DEPLOY-003/004/005; a successful
cross-build alone is insufficient. Multiarch runs exactly two native lanes:
`ubuntu-24.04` for amd64 and `ubuntu-24.04-arm` for arm64, with fail-fast off
and at most two concurrent runners. Each verifies its host architecture and
the existing default Docker driver, then builds and boots its matching image.
Neither Docker job starts a docker-container builder, registers binfmt, enables
privileged containers or changes daemon settings. GHA Docker cache exporters
are omitted because they would require a different builder configuration;
cold builds change cost, never verification scope. Published artifacts are
never identified by cache keys alone. See the native-runner amendment in
ADR-0008.

## Runtime isolation and failure/recovery

### Normal capability regression

The normal regression suite consumes the same source-bound binary and Web
artifacts. Its static lanes cover protocol input/output (four shards), API
business/authentication, browser subscription/settings, themes (four shards),
and the local SOCKS DNS handshake. At most four lanes run concurrently, each
with one browser worker and no retries. Test-level sharding requires
`fullyParallel`; tests still own independent processes, ports and databases.

The reviewed `tests/acceptance/normal-ci.json` catalogue binds every automated
case to exactly one lane. Discovery must match the complete catalogue before
execution and each lane's assigned cases afterwards. Adding/removing a test
requires updating this reviewed coverage projection. This is a static coverage
inventory, not selective scheduling or an alternate business authority.

The explicitly scoped CI normal result gate runs with `always()` and requires successful producer
and matrix job outcomes plus every expected case result from this source,
workflow run and attempt. Missing/duplicate/unknown cases, missing shards,
unexpected skips, expected-failure tests, retries, cancellation, timeout and
nonzero execution exits fail. Evidence contains exact commands, timestamps,
case outcomes, logs and screenshots; no database or keys are uploaded.

This automated scope is distinct from complete device/deployment acceptance.
Native GUI clients, real Android/iOS devices, authorized proxy connectivity,
and remaining unimplemented catalogue combinations stay blocked or not-run.
A browser mobile viewport is not an Android client. Full acceptance readiness
must report those gaps separately; a green automated gate cannot promote them.
The reviewed workflow has read-only repository access, no production secrets,
no publishing steps and no additional scheduled trigger.

Local browser suites use one preparation entrypoint before actual browser use.
HTTP-only and process-lifecycle tests in CI do not invoke this preparation. It checks
the installed Playwright packages against the lockfile, selects the registry's
full Chromium executable and verifies its exact version and revision metadata.
It then launches that explicit executable through Playwright, checks the
connected browser version, closes it and verifies that its bytes did not change
before exporting its path. Headless execution of full Chromium is allowed;
headless shell and system-browser substitution are not. The local preparation caller imposes a one-minute deadline, in addition to
the bounded version/launch probes.

Preparation consumes an already-present lawful asset through the standard
Playwright cache contract. Version policy, asset supply and acceptance remain
separate: no permitted provisioner is configured by this workflow, and it never
downloads a browser. The recorded executable SHA-256 identifies observed bytes;
it does not authenticate the publisher. An operator must establish the asset's
authorized source and integrity before placing it in an authorized environment.
Moving a refused download to another runner is not a remedy. Missing assets,
lock/version mismatches, launch or cleanup failures leave preparation and
dependent cases not-run and fail the job and final gate. No path is exported
before preparation succeeds. The independent installer and Node lifecycle checks stay in CI and do not
require a browser. Local browser children all receive the same prepared path.

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

## Local-browser evidence and full release boundary

`python3 scripts/ci/local_browser.py run|verify` owns the local-browser bundle.
Run on the final clean candidate commit, output outside tracked sources, and
supply independently hashed source-bound binary/Web producer manifests. The
runner must reject stale producers rather than attach new source labels to old
binaries. It records platform, exact commands, timeouts, exits, preparation,
raw report hashes and all exact cases. It sets the prepared executable path in
every browser child environment. Unknown cases, missing/duplicate cases,
skips/retries/global errors, source mutation and startup/cleanup failure fail.
The full release verifier requires a clean matching commit; development runs
on a dirty tree cannot certify release. This initially supports the existing
Linux producer manifest contract; macOS producer/runtime capability remains
separate NOT_RUN until a matching native contract/environment is verified.

The bundle is trusted execution evidence from the reviewed local runner, not
cryptographic proof against its owner. Its expected manifest digest comes from
separate operator review; an embedded self-hash or PASS string cannot certify
itself. No arbitrary URL fetch, browser acquisition or new storage integration
is introduced. Release `acceptance-gate` expects an already-approved bundle at
its documented input path plus the separately approved digest. This handoff is
currently unconfigured, so both tag and manual release stay blocked. Publication
and that handoff still require explicit concrete-content approval.
