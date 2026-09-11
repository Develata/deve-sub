# Milestone 8 — Deployment and Hardening

## Scope

Production deployment infrastructure, self-update mechanism, and performance
validation. M8 closes the deployment surface: Docker Compose for one-command
startup, an install script for bare-metal/VM deployment with systemd, multi-arch
Docker images (amd64 + arm64), a self-update CLI subcommand with signed binary
verification and automatic rollback, and a performance benchmark suite.

M8 is the final milestone. Backup (M11) and SSRF hardening (SEC-001..005, shipped
in M4) are already complete and are not re-delivered here.

## Dependency

M1 (Infrastructure) must be complete. The Dockerfile, health endpoints, CLI
framework, and migration infrastructure are prerequisites.

M7 (Probes and Detection) must be complete. All domain logic that M8's
deployment surface must host is in place.

M11 (Archive and Snapshot) must be complete. The self-update rollback path
uses the backup/restore infrastructure for data safety.

## Vertical slice

```text
docker compose up
    → builds (or pulls) the deve-sub image
    → runs migrations
    → starts the server
    → healthcheck passes within 60s

deve-sub update
    → checks GitHub Releases for a newer version
    → downloads the new binary + checksum file
    → verifies SHA-256 checksum
    → backs up the current binary
    → swaps in the new binary
    → restarts the service (if managed by systemd)
    → health-checks the new binary
    → on failure: rolls back to the previous binary

curl -fsSL https://raw.githubusercontent.com/.../install.sh | sh
    → downloads the latest release binary for the current platform
    → installs to /usr/local/bin/deve-sub
    → creates a systemd service unit
    → enables and starts the service
    → verifies health

cargo bench --bench parse_10k    # PERF-001
cargo bench --bench list_10k     # PERF-002 (API path, not browser)
cargo bench --bench generate     # PERF-003/004 (cached/uncached)
```

## Deliverables

- `docker-compose.yml` — single-file compose with volume, healthcheck, and
  migrate-then-serve entrypoint (DEPLOY-001).
- `scripts/install.sh` — idempotent install script that downloads the release
  binary, creates the data directory, writes a systemd unit, enables and starts
  the service, and verifies health (DEPLOY-002).
- Multi-arch CI: GitHub Actions job using `docker buildx` to build amd64 + arm64
  images in a single multi-platform build, pushed to the registry on tag
  (DEPLOY-003/004). DEPLOY-005 (Docker healthcheck) is already satisfied per
  ADR-0006; M8 adds a CI assertion that the healthcheck transitions to healthy
  on both architectures.
- `deve-sub update` CLI subcommand (UPDATE-001/002): downloads a new binary
  from GitHub Releases, verifies the SHA-256 checksum, backs up the current
  binary, atomically swaps, health-checks, and rolls back on failure.
- Criterion benchmark suite (PERF-001..006): 10k node parsing, 10k node listing,
  cached subscription delivery, uncached generation, concurrent download
  throughput, and a long-running soak test. Benchmarks produce baseline numbers;
  they are not pass/fail gates but observability tools.
- Release pipeline (Constraint #15): on tag, CI builds release binaries for
  linux/amd64 and linux/arm64, generates SHA-256 checksums, and produces an SBOM
  via `cargo cyclonedx`. The release also contains `deve-sub-web.tar.gz`, made
  from the verified frontend artifact of the same CI invocation. Checksums and
  the signed manifest cover both binaries and the frontend archive. The release
  signing secret must match the production verification key embedded in the CLI.
  Manual release preflight runs full CI, native/QEMU build smoke and artifact
  assembly without publication; tag pushes repeat these gates before publishing.

## Slicing

M8 is delivered in five slices:

1. **Docker Compose** (DEPLOY-001): `docker-compose.yml` with a migrate-then-serve
   entrypoint. Acceptance: `docker compose up` starts a healthy service.
2. **Multi-arch CI** (DEPLOY-003/004/005): buildx workflow for amd64 + arm64.
   Acceptance: CI builds both architectures; healthcheck passes on both.
3. **Install script** (DEPLOY-002): `scripts/install.sh` with systemd unit
   generation. Acceptance: script installs and starts the service on a Linux VM.
4. **Self-update CLI** (UPDATE-001/002): `deve-sub update` subcommand with
   download, checksum verification, backup, swap, health-check, and rollback.
   Acceptance: update succeeds and stays healthy; failed update rolls back.
5. **Performance benchmarks** (PERF-001..006): criterion bench suite covering
   parsing, listing, generation, delivery, concurrency, and soak. Acceptance:
   benchmarks run and produce baseline output.

## Architecture

### Docker Compose

```yaml
services:
  deve-sub:
    build: .            # or image: ghcr.io/develata/deve-sub:${TAG}
    ports: ["8080:8080"]
    volumes: ["deve-sub-data:/app/data"]
    healthcheck:
      test: ["CMD", "/app/deve-sub", "health", "live"]
      interval: 30s
      timeout: 3s
      start_period: 30s
      retries: 3
    restart: unless-stopped

volumes:
  deve-sub-data:
```

The compose file uses the existing Dockerfile. The `serve` command runs
migrations on startup if needed (via `verify_schema` which exits if schema is
stale — the operator runs `deve-sub migrate` first, or the entrypoint script
handles it). For zero-config startup, the entrypoint runs `migrate` then `serve`.

### Install script

```text
install.sh:
  1. detect OS (linux) and arch (amd64/arm64)
  2. download the latest release binary from GitHub Releases
  3. download the matching frontend archive and verify both SHA-256 checksums
  4. install binary to /usr/local/bin/deve-sub
  5. create /var/lib/deve-sub data directory
  6. generate /etc/systemd/system/deve-sub.service
  7. systemctl daemon-reload && systemctl restart deve-sub
  8. poll readiness and verify the running version, then enable the service
     (60 bounded attempts, up to 180 seconds)
```

The installer resolves `latest` to one release tag before downloading assets.
It installs the frontend under `/usr/local/share/deve-sub/web`; the systemd
unit passes that path explicitly to `serve --web-dist-dir`. Binary and frontend
backups are restored on installation failure, together with the prior unit
and running service state. Failed rollback retains backups for repair. The unit uses `Restart=on-failure`
and `After=network.target`. The installer still authenticates its initial
download through HTTPS; its checksum file alone is not a publisher signature.

The installer shares the binary updater's nonblocking filesystem lock. Recovery
backups live in a private persistent `/var/tmp` directory; a pending marker makes
an interrupted attempt fail clearly on the next invocation. Successful recovery
removes its own marker/backups; failed recovery retains both. This is not a
power-loss-atomic layout. Existing databases are not automatically migrated
on reinstall/upgrade: an incompatible schema must follow explicit backed-up
operator migration, rather than pretending binary rollback also rolls back SQL.
External downloads, archive processing and systemctl waits have fixed limits.
Unsupported whitespace/control characters and paths hidden by ProtectHome are
rejected before mutation; the unit specifies start/stop timeout and private umask.

### Self-update

```text
update:
  1. query GitHub Releases API for the latest release
  2. compare latest version vs current (CARGO_PKG_VERSION)
  3. if newer: download the binary for the current platform
  4. download checksums.txt; verify SHA-256 of the downloaded binary
  5. back up current binary to <binary>.bak
  6. write new binary to a temp path, then atomically rename
  7. if running under systemd: systemctl restart deve-sub
  8. poll health endpoint (30s timeout)
  9. if healthy: remove backup, report success
  10. if unhealthy: restore backup, restart, report failure (UPDATE-002)
```

The update command does NOT restart if the server is not managed by systemd —
it prints a message telling the operator to restart manually.

Signature verification (Constraint #15): the updater requires Ed25519 signed
manifests by default and rejects invalid or incomplete signatures. The explicit
`--allow-unsigned` option permits checksum-only updates from manually trusted
development sources when both signed assets are absent; it never bypasses a
bad or partial signature. Official releases are always signed. Before the first
production release, provision a dedicated signing key and replace the embedded
development verification key. Key provisioning is verified separately from
ordinary tests and never inferred from a successful build.

`deve-sub update` updates the native binary only. If the effective config has
`server.serve_web=true` (including the default), it refuses before network or
filesystem mutation unless `--binary-only` explicitly accepts frontend skew.
Invalid/missing explicit configuration fails closed. Headless configurations
may update without that override. Native Web installations should update both
assets through the installer; Docker deployments update the complete image.
`--force` permits reinstalling the current version; a downgrade additionally
requires `--allow-downgrade`. Neither flag bypasses publisher authentication.

Convergence decision: retain the existing installation layout for this round.
A versioned-directory transaction would also require adoption of existing
custom units/config, a durable recovery record, and a separate database upgrade
policy (the previous installer automatically migrated existing databases).
A pointer swap alone does not make those operations reversible. The explicit refusal closes the default
version-skew failure without introducing another deployment state machine.
A complete crash-recoverable binary-plus-Web update remains unimplemented;
installer rollback does not imply database migration rollback.

### Performance benchmarks

Benchmarks use Criterion with synthetic inputs at 100, 1,000 and 10,000 nodes.
The URI microbenchmark uses Trojan; round-trip/compatibility tests separately
cover protocol breadth. SQLite generation/reconcile benchmarks include the
real encrypted persistence pipeline.

- PERF-001: parse 10k Trojan URIs → measure throughput.
- PERF-002: list 10k nodes through the application query → measure latency.
- PERF-003: deliver a cached subscription → measure latency (cache hit path).
- PERF-004: generate a subscription with no cache → measure latency (full
  pipeline: selection → template → emit).
- PERF-005: concurrent subscription downloads (100 concurrent requests) →
  measure throughput.
- PERF-006: soak test (30-minute run with periodic refresh + delivery) →
  measure memory stability and error rate. This is a long-running test, not a
  Criterion bench. `scripts/perf/soak.py` defaults to 30 minutes; the ignored
  `apps/cli/tests/perf006_soak.rs` wrapper and CI use 90 seconds.

## Failure/recovery

- **Docker Compose failure**: the `restart: unless-stopped` policy restarts the
  container on crash. The healthcheck marks the container unhealthy after 3
  consecutive failures, which orchestration systems (Swarm, k8s) can act on.
- **Install script failure**: the script is idempotent — re-running it repairs
  the installation. If the health check fails after install, the script reports
  the failure and suggests `journalctl -u deve-sub` for diagnostics.
- **Self-update download failure**: the current binary is untouched. The error
  is reported. The user retries.
- **Self-update checksum failure**: the downloaded binary is deleted. The
  current binary is untouched. The error is reported.
- **Self-update health-check failure** (UPDATE-002): the new binary is replaced
  with the backup. The service is restarted with the old binary. The user is
  notified of the rollback. The failed binary is preserved as `<binary>.failed`
  for diagnostics.
- **Self-update with server not running**: the binary is swapped. The user is
  told to start the service manually. No health check is performed (no server
  to probe).
- **Multi-arch build failure**: if arm64 build fails but amd64 succeeds, the CI
  job fails as a whole (no partial release). The error is reported in the CI
  logs.

## Authority

- Docker base image and healthcheck: ADR-0006
- CLI subcommand names: AGENTS.md naming section (`update`)
- Release artifacts (SBOM, checksums, signatures): Constraint #15
- No `latest` image tag: Constraint #11
- Forward-only migration: Constraint #13
- Acceptance: DEPLOY-001..005, UPDATE-001/002, PERF-001..006

## Verification

- DEPLOY-001: `docker compose up -d` then poll `/health/live` until 200.
  Acceptance: DEPLOY-001.
- DEPLOY-002: run `install.sh` on a Linux VM, verify `systemctl status
  deve-sub` is active and `/health/live` returns 200. Acceptance: DEPLOY-002.
- DEPLOY-003/004: CI builds amd64 and arm64 images via buildx, runs each
  image, and verifies the healthcheck transitions to healthy. Acceptance:
  DEPLOY-003/004.
- DEPLOY-005: CI asserts the Docker HEALTHCHECK uses `deve-sub health live`
  and transitions to healthy within 60s. Already satisfied per ADR-0006; M8
  adds an explicit CI assertion. Acceptance: DEPLOY-005.
- UPDATE-001: run `deve-sub update` with a mock release that has a newer
  version; verify the binary is swapped and health check passes. Acceptance:
  UPDATE-001.
- UPDATE-002: run `deve-sub update` with a mock release whose binary fails
  the health check; verify the old binary is restored. Acceptance: UPDATE-002.
- PERF-001..006: `cargo bench` runs all benchmarks and produces baseline
  numbers. Run `DEVE_SUB_SOAK_SECONDS=1800 cargo test -- --ignored soak`
  for the 30-minute mode.
  Acceptance: PERF-001..006.
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all pass.
- `python3 scripts/check_docs.py` passes.

## Long-running task ownership (PERF-006, NODE-016)

The request-job supervisor admits at most 64 tracked jobs, reaps completed
jobs before admission and on periodic resource sampling, and reports panic
and cancellation counters without job-specific metric labels. Saturation or
shutdown returns 503; rejected durable jobs are moved to a terminal state.
Cancellation registration is owned by a drop guard captured before spawn,
so normal completion, panic, abort and admission rejection remove it. Shutdown
closes admission under the task-set mutex before draining; after the grace
period it aborts, then gives cancellation destructors a bounded drain. A
non-yielding future still requires process exit; no Rust task abort can
preempt arbitrary synchronous code. The CLI cancels both probe and refresh
registries and stops periodic workers before closing the SQLite pool.

## Supply-chain and executable I/O closure

CI uses reviewed full Action commit SHAs with an explicit Rust toolchain.
Compatibility validators are fixed-version, SHA-256 checked before extraction
and execution. `deny.toml` is the dependency policy owner; duplicate upstream
major versions are reported, while known vulnerabilities and incompatible
licenses block CI. The current paste exception is build-time-only and named
explicitly rather than disabling advisory checks.

Updater version probes are asynchronous, limited to 4 KiB output and five
seconds. systemctl has a 30-second client deadline; killing that client does
not cancel a systemd job. A restart failure restores the previous binary but
reports live service state as unverified. Restore accepts each authority file
once and only as a regular tar entry. Symlink/hardlink/duplicate entries fail.

## Frontend delivery verification

Dioxus registers the local stylesheet as a build asset; production dist must
contain HTML, JS, WASM and CSS. Browser acceptance verifies computed styles,
not only DOM class names. The 10k-node test asserts bounded rendered rows and
DOM count over repeated scrolling and filtering; observation timings are not
CI latency budgets.
