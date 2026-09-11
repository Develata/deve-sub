# ADR-0008: CI Parallelization and Verification-Preserving Artifact Sharing

- **Status**: Accepted
- **Date**: 2026-08-29

## Context

CI wall time was ~42 minutes. The monolithic `rust` job serialized
fmt → clippy → test → doc → openapi (~6m), then the critical path ran through
multiarch. Every push to `main` and every release tag paid this cost. Timing
data from run `32642394185` (pre-optimization) and runs `32889319311` /
`33246962802` (post-optimization) is the empirical basis.

Verification gates are non-negotiable constraints on any optimization:

- **B-19**: external validator tests are `#[ignore]`-gated so a green result
  means real validation occurred.
- **DS-AUD-B04**: a tag push must pass the *full* CI suite before any binary
  is built or released (fail-closed release gate via `workflow_call`).
- The full `cargo test --locked --all-targets --all-features` baseline is
  mandatory on every push to `main`; selective execution is reserved for a
  future PR-based workflow.

## Decision

1. **Shard the `rust` job** into independent jobs: `fmt-check`, `clippy`,
   `test` (9 per-crate shards via matrix, `fail-fast: false`), `test-doc`,
   `openapi-diff`, fanned in by a `rust-gate` job that downstream jobs depend
   on. Coverage is identical to the former serial run: every shard runs
   `cargo test --all-targets --all-features -p <crates>`, partitioned by
   crate. Lightweight leaf crates are grouped (`core`, `infra`) to stay within
   the 20-concurrent-job limit.
2. **Share artifacts only into `browser-e2e`.** `web-wasm` uploads
   `apps/web/dist`; a new `build-release` job uploads the release binary;
   `browser-e2e` downloads both and compiles nothing.
3. **`docker` and `multiarch` MUST keep building via the Dockerfile.** These
   jobs verify the Docker build pipeline itself; substituting a prebuilt
   binary would stop verifying what they exist to verify.
4. **Adopt the Docker GHA layer cache.** The `docker` job moves from a
   cacheless `docker build` to buildx with `cache-from/cache-to: type=gha`,
   reusing layers written by `multiarch` (same Dockerfile). `multiarch` loads
   its amd64 image from the GHA cache instead of a redundant full rebuild.
5. **Nightly scheduled full baseline** (cron). Every job in `ci.yml` runs on
   schedule; `multiarch` additionally runs on `schedule` so arm64 Docker
   layers stay warm.

## Fact correction recorded

The pre-optimization analysis assumed arm64 Docker builds ran `cargo build`
under QEMU emulation (~46m). The Dockerfile already cross-compiles via
`--platform=$BUILDPLATFORM` with `aarch64-unknown-linux-gnu` (see ADR-0006);
QEMU is needed only for the arm64 *runtime* stage (`apt-get` on
`debian:trixie-slim`). Consequences: per-arch job splitting offers no build
speedup (buildx already parallelizes per-platform), and the real multiarch
saving was eliminating the redundant amd64 rebuild in its verification step.

## Alternatives considered

1. **`paths`-filtered selective shard execution (Phase 2)** — deferred, not
   rejected. Selective execution is designed for PR scenarios; this repository
   works by direct push to `main` until the first tagged release, and the
   analysis itself mandates full runs on `main` pushes. Implementing it now
   would add fail-closed complexity for zero benefit and risk weakening the
   main gate.
2. **Inject the prebuilt release binary into Docker images (`COPY` instead of
   `cargo build` in the Dockerfile)** — rejected. It would stop `docker` and
   `multiarch` from verifying the Docker build pipeline, trading a
   verification invariant for speed. Performance must not weaken build
   verification (engineering priority order).
3. **Split `multiarch` into per-arch jobs** — rejected as redundant: buildx
   already builds both platforms in parallel within one job; the measured win
   came from removing the duplicated amd64 build, which decision 4 covers.
4. **`deve-sub verify` acceptance-orchestrator CLI** — deferred. A new CLI
   surface requires a contract and acceptance binding; GitHub Actions native
   mechanisms cover the current need.

## Consequences

- Wall time: ~42m → ~10m (cold caches) with a warm-cache critical path of
  ~7m. Observed job-level results (warm Docker cache): `docker` 14m37s → 32s,
  `multiarch` 14m24s → 42s, `browser-e2e` 15m14s → 1m19s (no compilation;
  downloads artifacts and runs Playwright).
- CI speed now depends on GHA cache warmth: rust-cache and Docker GHA cache
  expire after 7 days without use. The nightly cron keeps them warm and
  detects CI rot (runner image changes, action breakages) before a release
  tag hits it — `release.yml` gates releases on this suite, so silent rot
  would otherwise surface at the worst possible moment.
- Cache storage pressure grew (~15 rust-cache entries vs 1 before). Eviction
  can drop slow-to-rebuild entries; observed on 2026-08-29 when the
  `dioxus-cli` cache miss made `web-wasm` run 9m vs ~1m warm. This is
  accepted: cold runs remain correct, only slower.
- Test coverage semantics are unchanged: same commands, same flags,
  partitioned by `-p`. `fail-fast: false` trades a few runner-minutes for
  complete per-shard feedback in one cycle.

## 2026-09-09 implementation amendment

The current authority is `docs/plan/14-ci-verification.md`. Full verification
remains mandatory; a shadow planner records impact without skipping jobs.
Candidate CI binary builds may run alongside checks, while distribution
builds/signing/publication still wait for full release verification. A final
gate explicitly checks all jobs, and artifact consumers verify content and
source identity. Docker retains source builds with separate cache writers and
a shared frontend stage. Selective PR execution remains deferred pending
shadow evidence and per-shard receipts.

## 2026-09-11 convergence amendment

The current plan removes unused shadow selection and unsigned shard receipts.
Static matrix/command validation, GitHub job results, artifact provenance and
the final acceptance gate retain distinct responsibilities. Selective execution
remains deferred. Run 34532357779 at unchanged HEAD spent 8m39s installing dx;
the rust-cache key changed when an unrelated runner Rust toolchain changed.
An isolated Dioxus install-root cache addresses that measured invalidation.
Its future wall-time benefit must be measured on a new GitHub run, not inferred
from a local cache hit.

## References

- `.github/workflows/ci.yml` — WHY comments carry the change-set labels
  (Phase 1 sharding; 3-A WASM artifact, 3-B release binary, 3-C multiarch
  cache load, 3-F Docker GHA cache) that this ADR defines.
- `docs/adr/0006-docker-base-image-and-healthcheck.md` — Dockerfile
  `--platform=$BUILDPLATFORM` cross-compile design the fact correction rests on.
- Root `AGENTS.md` — Git strategy (direct `main` pushes before the first
  tagged release) and baseline check list.
- B-19, DS-AUD-B04 — fail-closed verification invariants preserved.
